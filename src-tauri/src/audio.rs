use std::io::Cursor;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

pub fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

pub fn resample_linear(mono: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || mono.is_empty() {
        return mono.to_vec();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = (mono.len() as f64 / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = mono[idx];
            let b = *mono.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

/// Returns the last `secs` seconds of `samples`, frame-aligned to `ch`
/// channels. Returns the whole slice if it's already shorter than the window.
pub fn tail(samples: &[f32], rate: u32, ch: u16, secs: f32) -> &[f32] {
    let ch = ch.max(1) as usize;
    let want = (rate as f32 * secs).round() as usize * ch;
    if want >= samples.len() {
        return samples;
    }
    let start = samples.len() - want;
    let start = start - (start % ch); // never split a frame
    &samples[start..]
}

pub const TARGET_RATE: u32 = 16_000;

pub fn prepare_wav(samples: &[f32], src_rate: u32, channels: u16) -> Vec<u8> {
    let mono = to_mono(samples, channels);
    let resampled = resample_linear(&mono, src_rate, TARGET_RATE);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("wav writer");
        for s in resampled {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(v).expect("wav sample");
        }
        writer.finalize().expect("wav finalize");
    }
    cursor.into_inner()
}

pub struct Recorder {
    buf: Arc<Mutex<Vec<f32>>>,
    meta: Mutex<Option<(u32, u16)>>,
    stop_tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            buf: Arc::new(Mutex::new(Vec::new())),
            meta: Mutex::new(None),
            stop_tx: Mutex::new(None),
        }
    }

    /// Starts capture on the default input device. `on_amplitude` receives an
    /// RMS value (0..~1) per audio callback; the caller throttles UI emits.
    pub fn start(&self, on_amplitude: Box<dyn Fn(f32) + Send>) -> Result<(), String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let mut stop_guard = self.stop_tx.lock().unwrap();
        if stop_guard.is_some() {
            return Err("already recording".into());
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "no input device".to_string())?;
        let config = device
            .default_input_config()
            .map_err(|e| format!("input config: {e}"))?;
        let rate = config.sample_rate().0;
        let channels = config.channels();

        self.buf.lock().unwrap().clear();
        *self.meta.lock().unwrap() = Some((rate, channels));

        let buf = self.buf.clone();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        *stop_guard = Some(stop_tx);
        drop(stop_guard);

        // cpal::Stream is !Send on macOS → own it on a dedicated thread.
        std::thread::spawn(move || {
            let stream = device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    let rms = (data.iter().map(|s| s * s).sum::<f32>()
                        / data.len().max(1) as f32)
                        .sqrt();
                    on_amplitude(rms);
                    buf.lock().unwrap().extend_from_slice(data);
                },
                |e| eprintln!("audio stream error: {e}"),
                None,
            );
            match stream {
                Ok(s) => {
                    if let Err(e) = s.play() {
                        let _ = ready_tx.send(Err(format!("play: {e}")));
                        return;
                    }
                    let _ = ready_tx.send(Ok(()));
                    let _ = stop_rx.recv(); // park until stop; dropping ends stream
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("build stream: {e}")));
                }
            }
        });

        let result = ready_rx
            .recv()
            .map_err(|_| "audio thread died".to_string())
            .and_then(|r| r);

        if result.is_err() {
            // Thread never confirmed the stream started (or died before sending) —
            // clear state so a retry doesn't hit "already recording" forever.
            self.stop_tx.lock().unwrap().take();
            *self.meta.lock().unwrap() = None;
        }

        result
    }

    /// Copy of everything captured so far (used for live partials).
    pub fn snapshot(&self) -> Option<(Vec<f32>, u32, u16)> {
        let (rate, ch) = (*self.meta.lock().unwrap())?;
        Some((self.buf.lock().unwrap().clone(), rate, ch))
    }

    pub fn duration_secs(&self) -> f32 {
        match *self.meta.lock().unwrap() {
            Some((rate, ch)) => {
                self.buf.lock().unwrap().len() as f32 / (rate as f32 * ch as f32)
            }
            None => 0.0,
        }
    }

    /// Stops capture and returns the full take.
    pub fn stop(&self) -> Option<(Vec<f32>, u32, u16)> {
        if let Some(tx) = self.stop_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
        let out = self.snapshot();
        *self.meta.lock().unwrap() = None;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_passthrough_and_stereo_average() {
        assert_eq!(to_mono(&[0.5, 0.5, 1.0], 1), vec![0.5, 0.5, 1.0]);
        assert_eq!(to_mono(&[0.0, 1.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn resample_halves_length_from_32k() {
        let src: Vec<f32> = vec![0.25; 3200];
        let out = resample_linear(&src, 32_000, 16_000);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|v| (v - 0.25).abs() < 1e-6));
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let src = vec![0.1_f32, 0.2, 0.3];
        assert_eq!(resample_linear(&src, 16_000, 16_000), src);
    }

    #[test]
    fn resample_downsample_exercises_zero_frac_interpolation() {
        // ratio = 32000/16000 = 2.0, out_len = floor(4/2.0) = 2
        // i=0: pos=0*2=0.0, idx=0, frac=0.0 -> a=0.0, b=1.0 -> 0.0+(1.0-0.0)*0.0 = 0.0
        // i=1: pos=1*2=2.0, idx=2, frac=0.0 -> a=2.0, b=3.0 -> 2.0+(3.0-2.0)*0.0 = 2.0
        let src = vec![0.0_f32, 1.0, 2.0, 3.0];
        let out = resample_linear(&src, 32_000, 16_000);
        assert_eq!(out, vec![0.0, 2.0]);
    }

    #[test]
    fn resample_upsample_exercises_fractional_interpolation() {
        // ratio = 16000/32000 = 0.5, out_len = floor(2/0.5) = 4
        // i=0: pos=0*0.5=0.0, idx=0, frac=0.0 -> a=0.0, b=1.0 -> 0.0+(1.0-0.0)*0.0 = 0.0
        // i=1: pos=1*0.5=0.5, idx=0, frac=0.5 -> a=0.0, b=1.0 -> 0.0+(1.0-0.0)*0.5 = 0.5
        // i=2: pos=2*0.5=1.0, idx=1, frac=0.0 -> a=1.0, b=mono.get(2) missing -> b=a=1.0 -> 1.0
        // i=3: pos=3*0.5=1.5, idx=1, frac=0.5 -> a=1.0, b=a=1.0 -> 1.0+(1.0-1.0)*0.5 = 1.0
        let src = vec![0.0_f32, 1.0];
        let out = resample_linear(&src, 16_000, 32_000);
        assert_eq!(out, vec![0.0, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn wav_has_riff_header_and_correct_data_size() {
        let samples = vec![0.0_f32; 16_000]; // 1s @ 16k mono
        let wav = prepare_wav(&samples, 16_000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // 44-byte canonical header + 2 bytes per sample
        assert_eq!(wav.len(), 44 + 16_000 * 2);
    }

    #[test]
    fn tail_returns_whole_slice_when_shorter_than_window() {
        let samples = vec![1.0_f32, 2.0, 3.0, 4.0];
        let out = tail(&samples, 16_000, 1, 25.0);
        assert_eq!(out, &samples[..]);
    }

    #[test]
    fn tail_slices_last_n_seconds_frame_aligned_stereo() {
        // rate=4, ch=2 => 8 samples/sec; secs=1.0 => window of 4 frames (8 samples).
        let samples: Vec<f32> = (0..20).map(|i| i as f32).collect(); // 10 stereo frames
        let out = tail(&samples, 4, 2, 1.0);
        assert_eq!(out, &samples[12..20]);
        assert_eq!(out.len() % 2, 0);
    }

    #[test]
    fn tail_rounds_start_down_to_frame_boundary() {
        // ch=2 but samples.len() is odd (pathological input) — the computed
        // start must still land on an even index, never splitting a frame.
        let samples: Vec<f32> = (0..21).map(|i| i as f32).collect();
        let out = tail(&samples, 2, 2, 1.0); // window = round(2*1.0)=2 frames = 4 samples
        assert_eq!(out, &samples[16..21]);
    }
}
