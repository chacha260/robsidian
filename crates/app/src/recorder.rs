//! 音声録音: 既定のマイクから録り、止めたら WAV（16ビット PCM）にする。
//!
//! 録音中の音はメモリにためる（48kHz・モノラルで1分あたり約6MB）。

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Recorder {
    /// 落とすと録音が止まる
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<i16>>>,
    channels: u16,
    rate: u32,
    started: Instant,
    /// 録音を始めたときに開いていたノート（止めたらここに埋め込む）
    pub note: String,
    /// 録音中に起きたエラー（マイクが外されたなど）
    error: Arc<Mutex<Option<String>>>,
}

impl Recorder {
    pub fn start(note: String) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("マイク（録音できる入力機器）が見つかりません"))?;
        let config = device
            .default_input_config()
            .context("マイクの設定を読めません")?;
        let samples = Arc::new(Mutex::new(Vec::new()));
        let error = Arc::new(Mutex::new(None));
        let stream = match config.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, &config.config(), &samples, &error),
            SampleFormat::I16 => build::<i16>(&device, &config.config(), &samples, &error),
            SampleFormat::U16 => build::<u16>(&device, &config.config(), &samples, &error),
            SampleFormat::I32 => build::<i32>(&device, &config.config(), &samples, &error),
            SampleFormat::U8 => build::<u8>(&device, &config.config(), &samples, &error),
            other => return Err(anyhow!("対応していない音声の形式です: {other}")),
        }?;
        stream.play().context("録音を始められません")?;
        Ok(Self {
            _stream: stream,
            samples,
            channels: config.channels(),
            rate: config.sample_rate(),
            started: Instant::now(),
            note,
            error,
        })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok()?.clone()
    }

    /// 録音を止めて WAV のバイト列を返す
    pub fn finish(self) -> Result<Vec<u8>> {
        let Self {
            _stream,
            samples,
            channels,
            rate,
            ..
        } = self;
        drop(_stream);
        let samples = std::mem::take(
            &mut *samples
                .lock()
                .map_err(|_| anyhow!("録音の途中で失敗しました"))?,
        );
        wav(&samples, channels, rate)
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: &Arc<Mutex<Vec<i16>>>,
    error: &Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    i16: FromSample<T>,
{
    let samples = samples.clone();
    let error = error.clone();
    device
        .build_input_stream::<T, _, _>(
            *config,
            move |data: &[T], _| {
                if let Ok(mut buf) = samples.lock() {
                    buf.extend(data.iter().map(|&s| i16::from_sample(s)));
                }
            },
            move |e| {
                if let Ok(mut slot) = error.lock() {
                    *slot = Some(e.to_string());
                }
            },
            None,
        )
        .context("マイクから録音を始められません")
}

/// 16ビット PCM の WAV にする
pub fn wav(samples: &[i16], channels: u16, rate: u32) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut out, spec)?;
    for &s in samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_wav() {
        let samples: Vec<i16> = (0..4800)
            .map(|i| ((i % 100) * 300 - 15000) as i16)
            .collect();
        let bytes = wav(&samples, 1, 48_000).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        let reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert_eq!(reader.spec().channels, 1);
        let back: Vec<i16> = reader.into_samples().map(|s| s.unwrap()).collect();
        assert_eq!(back, samples);
    }
}
