use std::{fs::File, io::copy, time::Duration};

use anyhow::{anyhow, Result};
use reqwest::{header::CONTENT_TYPE, Client, Url};
use tokio::time::sleep;

enum Source {
    Platen,
    Feeder,
}

struct ScanSettingsInput {
    source: Source,
    resolution: String,
    docformat: String,
    colormode: String,
    extformat: Option<bool>,
}
fn build_scansettings_xml(
    source: &str,
    resolution: &str,
    docformat: &str,
    colormode: &str,
    extformat: Option<bool>,
) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    xml.push_str("<scan:ScanSettings xmlns:pwg=\"http://www.pwg.org/schemas/2010/12/sm\" ");
    xml.push_str("xmlns:scan=\"http://schemas.hp.com/imaging/escl/2011/05/03\">");
    xml.push_str("<pwg:Version>2.0</pwg:Version>");
    xml.push_str("<pwg:InputSource>");
    xml.push_str(source);
    xml.push_str("</pwg:InputSource>");
    xml.push_str("<pwg:ScanRegions>");
    xml.push_str("<pwg:ScanRegion>");
    xml.push_str("<pwg:ContentRegionUnits>escl:ThreeHundredthsOfInches</pwg:ContentRegionUnits>");
    xml.push_str("<pwg:Height>3507</pwg:Height>");
    xml.push_str("<pwg:Width>2550</pwg:Width>");
    xml.push_str("<pwg:XOffset>0</pwg:XOffset>");
    xml.push_str("<pwg:YOffset>0</pwg:YOffset>");
    xml.push_str("</pwg:ScanRegion>");
    xml.push_str("</pwg:ScanRegions>");
    xml.push_str("<pwg:DocumentFormat>");
    xml.push_str(docformat);
    xml.push_str("</pwg:DocumentFormat>");
    if extformat.is_some() {
        xml.push_str("<pwg:DocumentFormatExt>");
        xml.push_str(docformat);
        xml.push_str("</pwg:DocumentFormatExt>");
    }
    xml.push_str("<scan:ColorMode>");
    xml.push_str(colormode);
    xml.push_str("</scan:ColorMode>");
    xml.push_str("<scan:XResolution>");
    xml.push_str(resolution);
    xml.push_str("</scan:XResolution>");
    xml.push_str("<scan:YResolution>");
    xml.push_str(resolution);
    xml.push_str("</scan:YResolution>");
    xml.push_str("</scan:ScanSettings>");
    xml
}

pub async fn post_scanrequest(
    url: &str,
    source: &str,
    resolution: &str,
    docformat: &str,
    colormode: &str,
) -> Result<Url> {
    let post_url = format!("{}/ScanJobs", url);
    println!("Send request to {}", post_url);

    let request = build_scansettings_xml(source, resolution, docformat, colormode, None);
    println!("{}", request);

    let client = Client::new();
    let mut count = 0;

    loop {
        match send_post(&client, &post_url, &request).await {
            Ok(ScannerResponse::Success(url)) => return Ok(url),
            Ok(ScannerResponse::Busy) => {
                increase_retry_count(&mut count).await;
            }
            Err(e) => return Err(e),
        };
    }
}

enum ScannerResponse {
    Success(Url),
    Busy,
}

async fn send_post(client: &Client, post_url: &str, request: &str) -> Result<ScannerResponse> {
    let response = match client
        .post(post_url)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(request.to_string())
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => return Err(e.into()),
    };

    if response.status().is_success() {
        let location_url = response
            .headers()
            .get("location")
            .and_then(|header| header.to_str().ok())
            .and_then(|location_string| Url::parse(&format!("{}/", location_string)).ok());

        if let Some(url) = location_url {
            return Ok(ScannerResponse::Success(url));
        }
    }

    if response.status().as_u16() == 503 {
        return Ok(ScannerResponse::Busy);
    }
    return Err(anyhow!(response.status()));
}

async fn increase_retry_count(count: &mut i32) {
    println!(
        "Scanner seems busy (HTTP 503), waiting {} of 100 seconds",
        count
    );
    *count += 1;
    sleep(Duration::from_secs(1)).await;
    const MAX_RETRIES: i32 = 100;
    if *count >= MAX_RETRIES {
        panic!("Scanner is busy for too long");
    }
}

pub async fn fetch_result(
    location: Url,
    outfile: &str,
    multi: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    println!("{}", location);
    let mut count = 1;

    loop {
        let filename = determine_filename(multi, outfile, count);
        sleep(Duration::from_secs(1)).await;

        let response = match client.get(location.join("NextDocument")?).send().await {
            Ok(response) => response,
            Err(e) => {
                panic!("HTTP request failed: {}", e);
            }
        };

        println!("{}", response.status());
        if response.status().is_success() {
            let mut dest = File::create(filename)?;
            let content = response.bytes().await?;
            copy(&mut content.as_ref(), &mut dest)?;
            count += 1;
            if !multi {
                break;
            }
        } else if response.status().as_u16() == 404 {
            break;
        } else {
            panic!("Unexpected HTTP error: {}", response.status());
        }
    }
    Ok(())
}

fn determine_filename(multi: bool, outfile: &str, count: i32) -> String {
    let filename = if multi {
        let parts: Vec<&str> = outfile.split('.').collect();
        format!("{}-{}.{}", parts[0], count, parts[1])
    } else {
        outfile.to_string()
    };
    filename
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use std::{fs, path::Path};

    #[test]
    fn parse_url() {
        let url_as_string =
            String::from("http://192.168.2.38/eSCL/ScanJobs/b30a11f0-5834-11b2-8325-3c2af490c39a");
        assert_eq!(
            Url::parse(url_as_string.as_str()).unwrap().as_str(),
            "http://192.168.2.38/eSCL/ScanJobs/b30a11f0-5834-11b2-8325-3c2af490c39a"
        );
    }

    #[test]
    fn join_next_document() {
        let url =
            Url::parse("http://192.168.2.38/eSCL/ScanJobs/b30a11f0-5834-11b2-8325-3c2af490c39a/")
                .unwrap();
        assert_eq!(
            url.join("NextDocument").unwrap().as_str(),
            "http://192.168.2.38/eSCL/ScanJobs/b30a11f0-5834-11b2-8325-3c2af490c39a/NextDocument"
        );
    }

    #[tokio::test]
    async fn test_send_post_success() {
        let mut server = mockito::Server::new();

        let _m = server
            .mock("POST", "/")
            .with_header("content-type", "application/x-www-form-urlencoded")
            .with_body("request")
            .with_status(200)
            .with_header("location", "http://example.com")
            .create();

        let client = Client::new();
        let result = send_post(&client, server.url().as_str(), "request")
            .await
            .unwrap();

        match result {
            ScannerResponse::Success(url) => assert_eq!(url.as_str(), "http://example.com/"),
            _ => panic!("Unexpected result"),
        }
    }

    #[tokio::test]
    async fn test_send_post_busy() {
        let mut server = mockito::Server::new();

        let _m = server
            .mock("POST", "/")
            .with_header("content-type", "application/x-www-form-urlencoded")
            .with_body("request")
            .with_status(503)
            .create();

        let client = Client::new();
        let result = send_post(&client, server.url().as_str(), "request")
            .await
            .unwrap();

        match result {
            ScannerResponse::Busy => {}
            _ => panic!("Unexpected result"),
        }
    }

    #[tokio::test]
    async fn test_send_post_error() {
        let mut server = mockito::Server::new();

        let _m = server
            .mock("POST", "/")
            .with_header("content-type", "application/x-www-form-urlencoded")
            .with_body("request")
            .with_status(500)
            .create();

        let client = Client::new();
        let result = send_post(&client, server.url().as_str(), "request").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_result_success_single() {
        let mut server = mockito::Server::new();
        let _m = server
            .mock("GET", "/NextDocument")
            .with_status(200)
            .with_body("Hello, world!")
            .create();

        let url = Url::parse(server.url().as_str()).unwrap();
        let outfile = "test.txt";
        let multi = false;

        fetch_result(url, outfile, multi).await.unwrap();

        let contents = fs::read_to_string(outfile).unwrap();
        assert!(Path::new(outfile).exists());
        assert_eq!(contents, "Hello, world!");

        fs::remove_file(outfile).unwrap();
    }

    #[tokio::test()]
    async fn test_fetch_result_success_multi() {
        let mut server = mockito::Server::new();
        let _m1 = server
            .mock("GET", "/NextDocument")
            .with_status(200)
            .with_body("Hello, world!")
            .create();

        let _m2 = server
            .mock("GET", "/NextDocument")
            .with_status(200)
            .with_body("Goodbye, world!")
            .create();

        let _m3 = server
            .mock("GET", "/NextDocument")
            .with_status(404)
            .create();

        let url = Url::parse(server.url().as_str()).unwrap();
        let outfile = "test.txt";
        let multi = true;

        fetch_result(url, outfile, multi).await.unwrap();

        let contents1 = fs::read_to_string("test-1.txt").unwrap();
        let contents2 = fs::read_to_string("test-2.txt").unwrap();
        assert_eq!(contents1, "Hello, world!");
        assert_eq!(contents2, "Goodbye, world!");

        fs::remove_file("test-1.txt").unwrap();
        fs::remove_file("test-2.txt").unwrap();
    }

    #[tokio::test]
    async fn test_fetch_result_404() {
        let mut server = mockito::Server::new();
        let _m = server
            .mock("GET", "/NextDocument")
            .with_status(404)
            .create();

        let url = Url::parse(server.url().as_str()).unwrap();
        let outfile = "test.txt";
        let multi = false;

        fetch_result(url, outfile, multi).await.unwrap();

        assert!(!Path::new(outfile).exists());
    }

    #[tokio::test]
    #[ignore]
    async fn test_fetch_result_unexpected_error() {
        let mut server = mockito::Server::new();
        let _m = server
            .mock("GET", "/NextDocument")
            .with_status(500)
            .create();

        let url = Url::parse(server.url().as_str()).unwrap();
        let outfile = "test.txt";
        let multi = false;

        let result = fetch_result(url, outfile, multi).await;

        assert!(result.is_err());
    }
}
