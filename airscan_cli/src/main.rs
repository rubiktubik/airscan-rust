use airscan_lib::{fetch_result, post_scanrequest};
use clap::Parser;
use url::Url;

/// Scan from an AirScan capable scanner
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL of the scannner, defaults to http://brother/eSCL
    #[arg(short, long, default_value = "http://192.168.2.38/eSCL")]
    url: String,

    /// Source
    #[arg(short, long, default_value = "Feeder")]
    source: String,

    /// Resolution
    #[arg(short, long, default_value = "300")]
    resolution: String,

    /// Format jpg or pdf
    #[arg(short, long, default_value = "pdf")]
    format: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Args::parse();

    let multifile = if &opt.source == "Feeder" && opt.format != "pdf" {
        true
    } else {
        false
    };

    let result = post_scanrequest(
        &opt.url,
        &opt.source,
        &opt.resolution,
        &format!("application/{}", opt.format),
        "RGB24",
    )
    .await?;

    let valid_result_url =
        Url::parse(result.as_str()).unwrap_or_else(|_error| panic!("unvalid scanner data"));

    let outfile = format!("scan.{}", opt.format);

    fetch_result(valid_result_url, &outfile, multifile).await?;

    Ok(())
}
