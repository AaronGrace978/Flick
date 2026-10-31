//! Continuous 20 kHz beacon. Start with the system volume low.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{error::Error, f64::consts::TAU, io};

const FREQUENCY: f64 = 20_000.0;

/// Builds an allocation-free callback, with a 100 ms fade at startup and shutdown.
fn stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let channels = config.channels as usize;
    let step = TAU * FREQUENCY / config.sample_rate.0 as f64;
    let mut phase = 0.0_f64;
    let mut gain = 0.0_f32;
    let ramp = 1.0 / (config.sample_rate.0 as f32 * 0.1);
    device.build_output_stream(config, move |data: &mut [T], _| {
        let target = if stop.load(std::sync::atomic::Ordering::Relaxed) { 0.0 } else { 1.0 };
        for frame in data.chunks_mut(channels) {
            gain += (target - gain).clamp(-ramp, ramp);
            let sample = T::from_sample((phase.sin() as f32) * 0.1 * gain);
            frame.fill(sample);
            phase += step;
            if phase >= TAU { phase -= TAU; }
        }
    }, |e| eprintln!("Audio stream error: {e}"), None)
}

fn main() -> Result<(), Box<dyn Error>> {
    let device = cpal::default_host().default_output_device().ok_or("No output device")?;
    // Prefer 48 kHz; reject sample rates without usable headroom above 20 kHz.
    let mut choices: Vec<_> = device.supported_output_configs()?.filter(|c| {
        c.max_sample_rate().0 >= 44_100 && matches!(c.sample_format(),
            cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16)
    }).collect();
    choices.sort_by_key(|c| (! (c.min_sample_rate().0 <= 48_000 && c.max_sample_rate().0 >= 48_000), c.channels()));
    let range = choices.into_iter().next().ok_or("No suitable >=44.1 kHz output format")?;
    let rate = 48_000_u32.clamp(range.min_sample_rate().0.max(44_100), range.max_sample_rate().0);
    let supported = range.with_sample_rate(cpal::SampleRate(rate));
    let config = supported.config();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let output = match supported.sample_format() {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config, stop.clone())?,
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config, stop.clone())?,
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config, stop.clone())?,
        _ => return Err("Unsupported sample format".into()),
    };
    output.play()?;
    println!("20 kHz beacon at {rate} Hz, amplitude 0.1. Press Enter to stop with fade-out.");
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(150));
    drop(output);
    Ok(())
}
