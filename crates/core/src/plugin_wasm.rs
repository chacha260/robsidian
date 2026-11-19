//! WASM のプラグイン（フェーズE3）: wasmi で、箱（サンドボックス）の中で動かす。
//!
//! WASI は与えないので、ファイル・ネットワーク・時計・プロセスには直接触れない。外とのやりとりは
//! 取り込み関数 `robsidian.call` / `notify` / `log` だけで、`call` は `plugin.toml` の `permissions`
//! で許したものだけを通す。計算量（fuel）とメモリにも上限を付ける。
//!
//! 約束（ABI）: 文字列は UTF-8 の JSON。プラグインは `memory`・`alloc(len) -> ptr`・
//! `handle(ptr, len) -> (ptr << 32 | len)` を出す。詳しくは docs/ROADMAP.md のフェーズE3。

use crate::plugin::{HostCall, Permission, Plugin, Request, Resident, Response, TIMEOUT};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use wasmi::{
    Caller, Engine, Extern, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder,
};

/// 1回の依頼で使える計算量（おおよその命令数）
const FUEL: u64 = 2_000_000_000;
/// 使えるメモリ
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;

type Replies = Arc<Mutex<HashMap<u64, Sender<Result<Value>>>>>;

/// 取り込み関数から見える状態
struct HostState {
    name: String,
    permissions: Vec<Permission>,
    calls: Sender<HostCall>,
    replies: Replies,
    next_id: Arc<AtomicU64>,
    limits: StoreLimits,
}

/// 読み込んだ WASM のプラグイン。実行は専用のスレッドで行う（落とすと止まる）
pub struct WasmPlugin {
    requests: Sender<(Request, Sender<Result<Response>>)>,
    calls: Receiver<HostCall>,
    replies: Replies,
    dead: Arc<AtomicBool>,
}

fn pack(ptr: u32, len: u32) -> i64 {
    ((u64::from(ptr) << 32) | u64::from(len)) as i64
}

fn unpack(v: i64) -> (u32, u32) {
    let v = v as u64;
    ((v >> 32) as u32, v as u32)
}

fn memory_of(caller: &Caller<'_, HostState>) -> Result<Memory, wasmi::Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmi::Error::new("プラグインが memory を出していません"))
}

fn read_str(caller: &Caller<'_, HostState>, ptr: i32, len: i32) -> Result<String, wasmi::Error> {
    let memory = memory_of(caller)?;
    let mut buf = vec![0u8; len.max(0) as usize];
    memory
        .read(caller, ptr as u32 as usize, &mut buf)
        .map_err(|_| wasmi::Error::new("プラグインのメモリの外を読もうとしました"))?;
    String::from_utf8(buf).map_err(|_| wasmi::Error::new("UTF-8 ではない文字列です"))
}

/// プラグインの `alloc` で場所を取って書き込み、(ptr << 32 | len) を返す
fn write_out(caller: &mut Caller<'_, HostState>, bytes: &[u8]) -> Result<i64, wasmi::Error> {
    let alloc = caller
        .get_export("alloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| wasmi::Error::new("プラグインが alloc を出していません"))?
        .typed::<i32, i32>(&*caller)?;
    let ptr = alloc.call(&mut *caller, bytes.len() as i32)? as u32;
    let memory = memory_of(caller)?;
    memory
        .write(&mut *caller, ptr as usize, bytes)
        .map_err(|_| wasmi::Error::new("プラグインのメモリに書けません"))?;
    Ok(pack(ptr, bytes.len() as u32))
}

fn link(engine: &Engine) -> Result<Linker<HostState>> {
    let mut linker = Linker::new(engine);
    linker.func_wrap(
        "robsidian",
        "call",
        |mut caller: Caller<'_, HostState>,
         mp: i32,
         ml: i32,
         pp: i32,
         pl: i32|
         -> Result<i64, wasmi::Error> {
            let method = read_str(&caller, mp, ml)?;
            let params: Value =
                serde_json::from_str(&read_str(&caller, pp, pl)?).unwrap_or(Value::Null);
            let answer = ask(caller.data(), &method, params);
            let out = match answer {
                Ok(v) => json!({ "ok": v }),
                Err(e) => json!({ "error": format!("{e:#}") }),
            };
            write_out(&mut caller, out.to_string().as_bytes())
        },
    )?;
    linker.func_wrap(
        "robsidian",
        "notify",
        |caller: Caller<'_, HostState>,
         mp: i32,
         ml: i32,
         pp: i32,
         pl: i32|
         -> Result<(), wasmi::Error> {
            let method = read_str(&caller, mp, ml)?;
            let params: Value =
                serde_json::from_str(&read_str(&caller, pp, pl)?).unwrap_or(Value::Null);
            let _ = caller.data().calls.send(HostCall {
                id: None,
                method,
                params,
            });
            Ok(())
        },
    )?;
    linker.func_wrap(
        "robsidian",
        "log",
        |caller: Caller<'_, HostState>, p: i32, l: i32| -> Result<(), wasmi::Error> {
            let text = read_str(&caller, p, l)?;
            eprintln!("[{}] {text}", caller.data().name);
            Ok(())
        },
    )?;
    Ok(linker)
}

/// 問い合わせを外（アプリ・CLI）に渡して返事を待つ。権限の無いものはここで断る
fn ask(state: &HostState, method: &str, params: Value) -> Result<Value> {
    match Permission::needed_for(method) {
        None => bail!("未対応の問い合わせです: {method}"),
        Some(p) if !state.permissions.contains(&p) => {
            bail!(
                "権限がありません: {method}（plugin.toml の permissions に「{}」が要ります）",
                match p {
                    Permission::Read => "read",
                    Permission::Write => "write",
                }
            )
        }
        Some(_) => {}
    }
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    state
        .replies
        .lock()
        .map_err(|_| anyhow!("返事の受け口が壊れています"))?
        .insert(id, tx);
    state
        .calls
        .send(HostCall {
            id: Some(json!(id)),
            method: method.to_string(),
            params,
        })
        .map_err(|_| anyhow!("Robsidian に問い合わせを渡せません"))?;
    rx.recv_timeout(TIMEOUT)
        .map_err(|_| anyhow!("問い合わせの返事がありません: {method}"))?
}

/// 依頼を1つ処理する（WASM の handle を呼ぶ）。WASM が止まった（trap）ら、2つ目が true
fn handle(
    store: &mut Store<HostState>,
    instance: &wasmi::Instance,
    req: &Request,
    fuel: u64,
) -> (Result<Response>, bool) {
    match call_handle(store, instance, req, fuel) {
        Ok(r) => (r, false),
        Err(e) => (Err(e), true),
    }
}

/// 外側の Err は WASM が止まったとき、内側の Err は返事の形の誤り
fn call_handle(
    store: &mut Store<HostState>,
    instance: &wasmi::Instance,
    req: &Request,
    fuel: u64,
) -> Result<Result<Response>> {
    store.set_fuel(fuel)?;
    let memory = instance
        .get_memory(&*store, "memory")
        .ok_or_else(|| anyhow!("プラグインが memory を出していません"))?;
    let alloc = instance.get_typed_func::<i32, i32>(&*store, "alloc")?;
    let handle = instance.get_typed_func::<(i32, i32), i64>(&*store, "handle")?;
    let input = serde_json::to_vec(req)?;
    let ptr = alloc.call(&mut *store, input.len() as i32)?;
    memory
        .write(&mut *store, ptr as u32 as usize, &input)
        .map_err(|_| anyhow!("プラグインのメモリに書けません"))?;
    let out = handle
        .call(&mut *store, (ptr, input.len() as i32))
        .map_err(|e| {
            if e.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
                anyhow!("計算が長すぎるので止めました")
            } else {
                anyhow!("プラグインが止まりました: {e}")
            }
        })?;
    let (optr, olen) = unpack(out);
    let mut buf = vec![0u8; olen as usize];
    memory
        .read(&*store, optr as usize, &mut buf)
        .map_err(|_| anyhow!("プラグインの返事を読めません"))?;
    Ok(String::from_utf8(buf)
        .context("返事が UTF-8 ではありません")
        .and_then(|text| crate::plugin::parse_response(req.kind, &text)))
}

impl WasmPlugin {
    /// `.wasm` を読み込み、専用のスレッドで待たせる
    pub fn load(plugin: &Plugin) -> Result<Self> {
        let file = plugin
            .manifest
            .wasm
            .clone()
            .unwrap_or_else(|| "plugin.wasm".into());
        let bytes = std::fs::read(plugin.dir.join(&file))
            .with_context(|| format!("{file} を読めません"))?;
        Self::from_bytes(
            &plugin.manifest.name,
            &plugin.manifest.permissions,
            &bytes,
            FUEL,
        )
    }

    /// `fuel` は1回の依頼で使える計算量
    pub fn from_bytes(
        name: &str,
        permissions: &[Permission],
        bytes: &[u8],
        fuel: u64,
    ) -> Result<Self> {
        let mut config = wasmi::Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, bytes).context("WASM として読めません")?;
        let (calls_tx, calls) = mpsc::channel();
        let replies: Replies = Arc::default();
        let state = HostState {
            name: name.to_string(),
            permissions: permissions.to_vec(),
            calls: calls_tx,
            replies: replies.clone(),
            next_id: Arc::new(AtomicU64::new(1)),
            limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
        };
        let mut store = Store::new(&engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(fuel)?;
        let linker = link(&engine)?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .context("WASM を始められません（取り込み関数か、出している関数が合いません）")?;
        // 必要な関数があるかを先に確かめる
        instance
            .get_typed_func::<i32, i32>(&store, "alloc")
            .context("alloc(len: i32) -> i32 を出してください")?;
        instance
            .get_typed_func::<(i32, i32), i64>(&store, "handle")
            .context("handle(ptr: i32, len: i32) -> i64 を出してください")?;
        let (requests, rx) = mpsc::channel::<(Request, Sender<Result<Response>>)>();
        let dead = Arc::new(AtomicBool::new(false));
        let dead2 = dead.clone();
        std::thread::spawn(move || {
            for (req, reply) in rx {
                let (r, trapped) = handle(&mut store, &instance, &req, fuel);
                let _ = reply.send(r);
                // 止まった（trap）ら、中の状態が壊れているかもしれないので、それ以降は使わない
                // （アプリは次に使うときに読み込み直す）
                if trapped {
                    break;
                }
            }
            dead2.store(true, Ordering::Relaxed);
        });
        Ok(Self {
            requests,
            calls,
            replies,
            dead,
        })
    }
}

impl Resident for WasmPlugin {
    fn request(&self, req: &Request) -> Receiver<Result<Response>> {
        let (tx, rx) = mpsc::channel();
        if let Err(mpsc::SendError((_, tx))) = self.requests.send((req.clone(), tx)) {
            let _ = tx.send(Err(anyhow!("プラグインは止まっています")));
        }
        rx
    }

    fn calls(&self) -> Vec<HostCall> {
        self.calls.try_iter().collect()
    }

    fn reply(&self, id: Value, result: Result<Value>) {
        let Some(id) = id.as_u64() else {
            return;
        };
        if let Some(tx) = self.replies.lock().ok().and_then(|mut r| r.remove(&id)) {
            let _ = tx.send(result);
        }
    }

    fn alive(&self) -> bool {
        !self.dead.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{request_blocking, Action, Kind, NoteInfo};

    /// 試し用のプラグイン（WAT で書く）。
    /// - 依頼が来たら `ui/notice` を送り、`vault/read` で問い合わせる。返事が `{"ok"…` なら「ok」、
    ///   `{"error"…` なら「denied」を通知として返す
    /// - コマンドの依頼（JSON の17文字目が c）なら止まらない（計算量の上限で止まることを確かめる）
    const WAT: &str = r#"
(module
  (import "robsidian" "call" (func $call (param i32 i32 i32 i32) (result i64)))
  (import "robsidian" "notify" (func $notify (param i32 i32 i32 i32)))
  (memory (export "memory") 2)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 0) "vault/read")
  (data (i32.const 16) "{\"path\":\"a\"}")
  (data (i32.const 32) "ui/notice")
  (data (i32.const 48) "{\"text\":\"hi\"}")
  (data (i32.const 64) "{\"actions\":[{\"type\":\"notice\",\"text\":\"ok\"}]}")
  (data (i32.const 128) "{\"actions\":[{\"type\":\"notice\",\"text\":\"denied\"}]}")
  (func (export "alloc") (param $len i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $p))
  (func (export "handle") (param $ptr i32) (param $len i32) (result i64)
    (local $r i64)
    (if (i32.eq (i32.load8_u (i32.add (local.get $ptr) (i32.const 17))) (i32.const 99))
      (then (loop $l (br $l))))
    (call $notify (i32.const 32) (i32.const 9) (i32.const 48) (i32.const 13))
    (local.set $r (call $call (i32.const 0) (i32.const 10) (i32.const 16) (i32.const 12)))
    ;; 返事の3文字目（{"ok" の o）
    (if (result i64)
      (i32.eq (i32.load8_u (i32.add (i32.wrap_i64 (i64.shr_u (local.get $r) (i64.const 32))) (i32.const 2))) (i32.const 111))
      (then (i64.or (i64.shl (i64.const 64) (i64.const 32)) (i64.const 43)))
      (else (i64.or (i64.shl (i64.const 128) (i64.const 32)) (i64.const 47)))))
)
"#;

    fn setup() -> (tempfile::TempDir, crate::Vault, Request) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "本文").unwrap();
        let v = crate::Vault::open(dir.path()).unwrap();
        let mut req = Request::new(Kind::Panel, "p", dir.path());
        req.note = Some(NoteInfo {
            path: "a.md".into(),
            ..NoteInfo::default()
        });
        (dir, v, req)
    }

    #[test]
    fn runs_wasm_with_permissions() {
        let wasm = wat::parse_str(WAT).unwrap();
        let (_d, mut v, req) = setup();
        let notice = |t: &str| Action::Notice { text: t.into() };

        // 権限あり: 問い合わせが通る。通知（ui/notice）も届く
        let p = WasmPlugin::from_bytes("t", &[Permission::Read], &wasm, FUEL).unwrap();
        let r = request_blocking(&p, &req, &mut v).unwrap();
        assert_eq!(r.actions, [notice("hi"), notice("ok")]);
        // 状態を持ったまま、2回目も答える
        let r = request_blocking(&p, &req, &mut v).unwrap();
        assert_eq!(r.actions, [notice("hi"), notice("ok")]);

        // 権限なし: 問い合わせは断られる（外へは渡らない）
        let p = WasmPlugin::from_bytes("t", &[], &wasm, FUEL).unwrap();
        let r = request_blocking(&p, &req, &mut v).unwrap();
        assert_eq!(r.actions, [notice("hi"), notice("denied")]);
        assert!(p.alive());
    }

    #[test]
    fn stops_runaway_plugins() {
        let wasm = wat::parse_str(WAT).unwrap();
        let (dir, mut v, _) = setup();
        let p = WasmPlugin::from_bytes("t", &[Permission::Read], &wasm, 1_000_000).unwrap();
        let req = Request::new(Kind::Command, "x", dir.path());
        let e = request_blocking(&p, &req, &mut v).unwrap_err();
        assert!(format!("{e:#}").contains("計算が長すぎる"), "{e:#}");
        // 止まったプラグインは使わない
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!p.alive());
        assert!(WasmPlugin::from_bytes("t", &[], b"not wasm", FUEL).is_err());
        // 必要な関数を出していない
        let bare = wat::parse_str("(module (memory (export \"memory\") 1))").unwrap();
        let e = WasmPlugin::from_bytes("t", &[], &bare, FUEL).err().unwrap();
        assert!(format!("{e:#}").contains("alloc"), "{e:#}");
    }

    #[test]
    fn asks_only_what_is_permitted() {
        let (calls_tx, calls) = mpsc::channel();
        let s = HostState {
            name: "t".into(),
            permissions: vec![Permission::Read],
            calls: calls_tx,
            replies: Arc::default(),
            next_id: Arc::new(AtomicU64::new(1)),
            limits: StoreLimits::default(),
        };
        let e = ask(&s, "vault/write", json!({})).unwrap_err();
        assert!(format!("{e}").contains("権限がありません"));
        assert!(ask(&s, "vault/explode", json!({})).is_err());
        assert!(calls.try_recv().is_err(), "断ったものは外へ渡さない");
    }
}
