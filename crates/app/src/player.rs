//! 音声の再生: WAV を読み、既定の出力機器で鳴らす（録音した `Recording ….wav` を聞くため）。
//!
//! 出力機器のチャンネル数とサンプリング周波数に合わせて、その場で変換する（周波数は線形補間）。

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Duration;

/// 再生できる形式か（拡張子で見る）
pub fn is_audio(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("wav"))
}

/// 読み込んだ音（チャンネルを交互に並べた16ビット。長い録音でもメモリを食いすぎないように）
pub struct Sound {
    pub samples: Vec<i16>,
    pub channels: usize,
    pub rate: u32,
}

impl Sound {
    pub fn read(path: &Path) -> Result<Self> {
        let reader = hound::WavReader::open(path).context("WAV ファイルを開けません")?;
        let spec = reader.spec();
        if spec.channels == 0 || spec.sample_rate == 0 {
            return Err(anyhow!("WAV ファイルの形式が正しくありません"));
        }
        let broken = || anyhow!("WAV ファイルが壊れています");
        let samples: Vec<i16> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .map(|s| s.map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16))
                .collect::<Result<_, _>>()
                .map_err(|_| broken())?,
            hound::SampleFormat::Int if spec.bits_per_sample <= 16 => reader
                .into_samples::<i16>()
                .collect::<Result<_, _>>()
                .map_err(|_| broken())?,
            hound::SampleFormat::Int => {
                let shift = spec.bits_per_sample - 16;
                reader
                    .into_samples::<i32>()
                    .map(|s| s.map(|s| (s >> shift) as i16))
                    .collect::<Result<_, _>>()
                    .map_err(|_| broken())?
            }
        };
        Ok(Self {
            samples,
            channels: spec.channels as usize,
            rate: spec.sample_rate,
        })
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.frames() as f64 / self.rate as f64)
    }

    /// `pos`（元の音のフレーム位置）から、出力の形式（-1.0〜1.0）で `out` を埋めて `pos` を進める。
    /// 最後まで鳴らし終えたら true（残りは無音）。
    pub fn render(
        &self,
        pos: &mut f64,
        out: &mut [f32],
        out_channels: usize,
        out_rate: u32,
    ) -> bool {
        let step = self.rate as f64 / out_rate as f64;
        let frames = self.frames();
        let sample =
            |frame: usize, ch: usize| f32::from(self.samples[frame * self.channels + ch]) / 32768.0;
        for frame in out.chunks_mut(out_channels.max(1)) {
            let i = *pos as usize;
            if i >= frames {
                frame.fill(0.0);
                continue;
            }
            let next = (i + 1).min(frames - 1);
            let t = (*pos - i as f64) as f32;
            for (c, slot) in frame.iter_mut().enumerate() {
                let at = |f: usize| {
                    if out_channels == 1 && self.channels > 1 {
                        // モノラルの出力には、全チャンネルの平均
                        (0..self.channels).map(|ch| sample(f, ch)).sum::<f32>()
                            / self.channels as f32
                    } else {
                        sample(f, c.min(self.channels - 1))
                    }
                };
                *slot = at(i) + (at(next) - at(i)) * t;
            }
            *pos += step;
        }
        *pos >= frames as f64
    }
}

/// 再生の状態（再生用のスレッドと分け合う）
#[derive(Default)]
struct State {
    /// 元の音で何フレーム目まで鳴らしたか
    position: AtomicU64,
    done: AtomicBool,
    /// 読み込めたら (サンプリング周波数, 長さ)
    info: OnceLock<(u32, Duration)>,
    error: Mutex<Option<String>>,
}

/// 音声を鳴らす。読み込みと再生は別のスレッドで行う（長い録音でも画面を止めないように）。
/// 落とすと止まる。
pub struct Player {
    /// 鳴らしているファイル（vault内のパス）
    pub path: String,
    state: Arc<State>,
    /// 落とすと、再生用のスレッドが音の出力を閉じて終わる
    _stop: mpsc::Sender<()>,
}

impl Player {
    pub fn start(abs: PathBuf, path: String) -> Self {
        let state = Arc::new(State::default());
        let (stop, stopped) = mpsc::channel::<()>();
        let shared = state.clone();
        std::thread::spawn(move || match open(&abs, &shared, &stopped) {
            // 出力は作ったスレッドで持ち続け、止める合図（Player が落ちる）まで待つ
            Ok(stream) => {
                let _ = stopped.recv();
                drop(stream);
            }
            Err(e) => {
                if let Ok(mut slot) = shared.error.lock() {
                    *slot = Some(format!("{e:#}"));
                }
                shared.done.store(true, Ordering::Relaxed);
            }
        });
        Self {
            path,
            state,
            _stop: stop,
        }
    }

    /// 読み込み中は None
    pub fn duration(&self) -> Option<Duration> {
        self.state.info.get().map(|&(_, d)| d)
    }

    pub fn position(&self) -> Duration {
        let Some(&(rate, duration)) = self.state.info.get() else {
            return Duration::ZERO;
        };
        let frames = self.state.position.load(Ordering::Relaxed);
        Duration::from_secs_f64(frames as f64 / rate as f64).min(duration)
    }

    /// 鳴らし終えたか、失敗した
    pub fn finished(&self) -> bool {
        self.state.done.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> Option<String> {
        self.state.error.lock().ok()?.clone()
    }

    /// `0:03 / 1:20`（読み込み中は「読み込み中…」）
    pub fn progress_label(&self) -> String {
        match self.duration() {
            Some(d) => format!("{} / {}", clock(self.position()), clock(d)),
            None => "読み込み中…".to_string(),
        }
    }

    /// 進み具合（0〜1）
    pub fn fraction(&self) -> f32 {
        self.duration().map_or(0.0, |d| {
            (self.position().as_secs_f32() / d.as_secs_f32().max(0.001)).clamp(0.0, 1.0)
        })
    }
}

/// 読み込んで、既定の出力機器で鳴らし始める
fn open(abs: &Path, state: &Arc<State>, stopped: &mpsc::Receiver<()>) -> Result<cpal::Stream> {
    let sound = Arc::new(Sound::read(abs)?);
    // 読み込んでいる間に止められていたら、鳴らさない
    if let Err(mpsc::TryRecvError::Disconnected) = stopped.try_recv() {
        return Err(anyhow!("止めました"));
    }
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow!("音を出す機器が見つかりません"))?;
    let config = device
        .default_output_config()
        .context("音を出す機器の設定を読めません")?;
    let stream_config = config.config();
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &stream_config, &sound, state),
        SampleFormat::I16 => build::<i16>(&device, &stream_config, &sound, state),
        SampleFormat::U16 => build::<u16>(&device, &stream_config, &sound, state),
        SampleFormat::I32 => build::<i32>(&device, &stream_config, &sound, state),
        SampleFormat::U8 => build::<u8>(&device, &stream_config, &sound, state),
        other => return Err(anyhow!("対応していない出力の形式です: {other}")),
    }?;
    let _ = state.info.set((sound.rate, sound.duration()));
    stream.play().context("再生を始められません")?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sound: &Arc<Sound>,
    state: &Arc<State>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    let mut pos = 0.0f64;
    let mut buf = Vec::new();
    let (sound, shared, on_error) = (sound.clone(), state.clone(), state.clone());
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |data: &mut [T], _| {
                buf.resize(data.len(), 0.0);
                let ended = sound.render(&mut pos, &mut buf, channels, rate);
                for (out, &s) in data.iter_mut().zip(&buf) {
                    *out = T::from_sample(s);
                }
                shared.position.store(pos as u64, Ordering::Relaxed);
                if ended {
                    shared.done.store(true, Ordering::Relaxed);
                }
            },
            move |e| {
                if let Ok(mut slot) = on_error.error.lock() {
                    *slot = Some(e.to_string());
                }
                on_error.done.store(true, Ordering::Relaxed);
            },
            None,
        )
        .context("音を出せません")
}

/// 再生位置の表示（`0:03 / 1:20`）
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_resamples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        // 2チャンネル・4フレーム（左は増え、右は0）
        let samples: Vec<i16> = [0, 0, 8192, 0, 16384, 0, 24576, 0].to_vec();
        std::fs::write(&path, crate::recorder::wav(&samples, 2, 4).unwrap()).unwrap();
        let sound = Sound::read(&path).unwrap();
        assert_eq!((sound.channels, sound.rate, sound.frames()), (2, 4, 4));
        assert_eq!(sound.duration(), Duration::from_secs(1));

        // モノラル・倍の周波数: 左右の平均を線形補間する
        let mut pos = 0.0;
        let mut out = [0.0f32; 4];
        assert!(!sound.render(&mut pos, &mut out, 1, 8));
        assert_eq!(out, [0.0, 0.0625, 0.125, 0.1875]);
        // ステレオ・同じ周波数: そのまま。最後まで鳴らしたら残りは無音
        let mut pos = 0.0;
        let mut out = [9.0f32; 10];
        assert!(sound.render(&mut pos, &mut out, 2, 4));
        assert_eq!(&out[..8], &[0.0, 0.0, 0.25, 0.0, 0.5, 0.0, 0.75, 0.0]);
        assert_eq!(&out[8..], &[0.0, 0.0]);

        assert!(is_audio("x/Recording 1.WAV") && !is_audio("a.mp3"));
        assert_eq!(clock(Duration::from_secs(83)), "1:23");
    }
}
