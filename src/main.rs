use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::Term;
use indicatif::{ProgressBar, ProgressStyle};
use std::{env, net::IpAddr, time::Duration};
use tapo::{ApiClient, PlugEnergyMonitoringHandler};
use textplots::{Chart, LabelBuilder, LabelFormat, Plot, Shape};
use tokio::time::sleep;

/// Empirically estimated maximum update-rate of the Tapo 'current power' reading.
/// Querying the device more frequently than this is pointless.
const TAPO_TEMPORAL_RESOLUTION: Duration = Duration::from_secs(1);

// How many samples we take for a single measurement.
const MEASUREMENT_SAMPLE_COUNT: usize = 10;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let username = env::var("TAPO_USERNAME")
        .context("Getting Tapo username from TAPO_USERNAME environment variable")?;
    let password = env::var("TAPO_PASSWORD")
        .context("Getting Tapo password from TAPO_PASSWORD environment variable")?;

    let device = ApiClient::new(username, password)
        .p115(&args.ip.to_string())
        .await
        .context("Connecting to the device")?;

    match args.command {
        TapoCommand::Measure => {
            let samples = get_samples(device).await?;
            print_stats(&samples);
        }
        TapoCommand::Monitor => monitor(device).await?,
    };

    Ok(())
}

async fn get_samples(device: PlugEnergyMonitoringHandler) -> Result<Vec<u64>> {
    let progress_bar_style = ProgressStyle::with_template(
        "obtaining samples... [{elapsed}] {bar:40.cyan/blue} {pos:>7}/{len:7}",
    )
    .expect("valid style");
    let progress_bar =
        ProgressBar::new(MEASUREMENT_SAMPLE_COUNT as u64).with_style(progress_bar_style);

    let mut samples = Vec::new();
    for _ in 0..MEASUREMENT_SAMPLE_COUNT {
        samples.push(device.get_current_power().await?.current_power);
        progress_bar.inc(1);
        sleep(TAPO_TEMPORAL_RESOLUTION).await;
    }

    progress_bar.finish_and_clear();

    Ok(samples)
}

fn print_stats(samples: &Vec<u64>) {
    let max = samples.iter().max().expect("we obtained samples");
    let min = samples.iter().min().expect("we obtained samples");

    let len = samples.len() as f32;
    let samples_f32 = samples.iter().map(|sample| *sample as f32);
    let mean = samples_f32.clone().sum::<f32>() / len;
    let variance: f32 = samples_f32
        .map(|sample| (sample - mean).powi(2))
        .sum::<f32>()
        / len;
    let standard_deviation = variance.sqrt();

    println!("avg: {mean:.1} W +-{standard_deviation:.1} W");
    println!("min: {min} W");
    println!("max: {max} W");
    println!("samples: {:?}", samples);
}

// Inspired by https://github.com/loony-bean/textplots-rs/blob/master/examples/liveplot.rs.
async fn monitor(device: PlugEnergyMonitoringHandler) -> Result<()> {
    const PLOT_WIDTH: usize = 100;

    let term = Term::stdout();
    term.clear_screen().unwrap();

    let mut samples: Vec<(f32, f32)> = Vec::new();
    loop {
        // Shift the collected samples.
        for sample in samples.iter_mut() {
            sample.0 -= 1.0;
        }
        if samples.len() == PLOT_WIDTH {
            samples.remove(0);
        }

        // Get the next sample.
        let sample = device.get_current_power().await?.current_power;
        samples.push((0., sample as f32));

        // Update the plot.
        term.move_cursor_to(0, 0).unwrap();
        Chart::new(200, 50, -(PLOT_WIDTH as f32), 0.0)
            .x_label_format(LabelFormat::Custom(Box::new(|ts| match ts {
                0.0 => "now".to_string(),
                ts => format!("{ts:.0} seconds"),
            })))
            .y_label_format(LabelFormat::Custom(Box::new(|watts| format!("{watts} W"))))
            .lineplot(&Shape::Steps(&samples))
            .nice();

        println!("current power: {sample}W");

        sleep(TAPO_TEMPORAL_RESOLUTION).await;
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    ip: IpAddr,
    #[command(subcommand)]
    command: TapoCommand,
}

#[derive(Subcommand, Clone, Debug)]
enum TapoCommand {
    /// Take a measurement of current power consumption over multiple samples.
    Measure,
    /// Continuously monitor momentary power consumption from your terminal.
    Monitor,
}
