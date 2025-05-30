use crate::types::{CipherTextValue, GatewayIngestMetricEvent};
use clap::Parser;
use client_auth::AuthToken;
use dotenv::dotenv;
use libmozaik_iot::{protect, DeviceState, ProtectionAlgorithm};
use reqwest::{header::DATE, Response};
use std::{
    env,
    error::Error,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    thread,
    time::{self, SystemTime, UNIX_EPOCH},
};
use types::IngestMetricEvent;

pub mod types;

/*
dataset_description.txt

Description for the ecg_dataset.txt file containing samples for ML inference for the heartbeat use-case of MOZAIK:

The first line of the file contains an integer X representing the number of samples in the file.
The second line of the file contains an integer Y representing the vector length of the samples.
From the third line onwards the samples are listed, separated by a line break (each sample is written in a new line). Hence, the file contains X samples of length Y, each in a new line.
 - each sample corresponds to a vector of Y numbers in floating point representation, separated by a space (" ").

 Conversion to representation needed for computing in MPC:
 In MPC we work in integer arithmetic, with fixed-point representation. For now we will fix the fixed-point precision to 8 bits, and the integer part will be represented by the remaining 56 bits.
 To convert number x to the desired format, compute f(x) = floor(x . 2^8). In other words, the input is multiplied by 2^f, where f denotes the fixed-point precision. Afterwards, a floor function is applied, a floor function in this case means rounding down to the nearest integer (= cutting the decimal part off).
 Further, the resulting 64-bit integer is encoded in 8 bytes in little-endian format with the least significant byte representing the decimal part.
*/

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Whether to use the gateway or not. Default false.
    #[arg(short, long, default_value_t = false)]
    gateway: bool,

    /// When using the gateway, is the gateway or the IoT device responsible for authenticating with MOZAIK? If this flag is present, the gateway will authenticate instead of the IoT device. Default false.
    #[arg(short = 'a', long, default_value_t = false)]
    gateway_authenticate: bool,

    /// Time between ingestion in milliseconds
    #[arg(short, long, default_value_t = 1000)]
    interval: u64,

    /// Limit amount of samples to ingest
    #[arg(short, long, default_value_t = 1000)]
    count: u128,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Args
    let args = Args::parse();

    // Env
    dotenv().ok();

    let ingest_endpoint = if args.gateway {
        env::var("GATEWAY_ENDPOINT").unwrap()
    } else {
        env::var("INGEST_ENDPOINT").unwrap()
    };

    let client_id = env::var("CLIENT_ID").unwrap();
    let client_secret = env::var("CLIENT_SECRET").unwrap();
    let auth_endpoint = env::var("AUTH_ENDPOINT").unwrap();
    let token_endpoint = env::var("TOKEN_ENDPOINT").unwrap();

    // Auth token
    let mut auth_token = AuthToken::new(
        client_id.clone(),
        client_secret,
        auth_endpoint,
        token_endpoint,
    )
    .await;

    // nonce + key
    let nonce = [
        0x73, 0x3f, 0x77, 0x3e, 0x1d, 0x5f, 0xa3, 0xdf, 0x5e, 0x05, 0x6b, 0xf5,
    ]; // this should be a fresh nonce

    let key = [
        0x8a, 0x47, 0xc0, 0x45, 0x16, 0x7b, 0x1a, 0xd4, 0x49, 0x46, 0x85, 0xa5, 0x20, 0xd0, 0xd6,
        0x9e,
    ]; // this should be a fresh device key

    let mut state = DeviceState::new(nonce, key);

    let dataset = File::open("../ecg_dataset.txt")?;
    let dataset_buff_reader = BufReader::new(dataset);

    let mut line_iterator = dataset_buff_reader.lines();

    if let Some(Ok(x)) = line_iterator.next() {
        println!("Amount of samples: {}.", &x);
    } else {
        panic!("Cannot read amount of samples.");
    };

    if let Some(Ok(y)) = line_iterator.next() {
        println!("Sample length: {}.", &y);
    } else {
        panic!("Cannot read sample length.");
    };

    let http_client = reqwest::Client::new();

    let bench_file_path = format!(
        "ingest_int-{}ms_c-{}_ingest-{}_auth-{}_time-{}.txt",
        args.interval,
        args.count,
        if args.gateway { "gateway" } else { "iot" },
        if args.gateway_authenticate {
            "gateway"
        } else {
            "iot"
        },
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
    );

    let mut bench_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(bench_file_path)?;

    writeln!(
        bench_file,
        "sample_read_micros,sample_encrypt_micros,sample_ingest_micros"
    )?;

    // Iterate over each sample in the dataset
    for (i, sample_line) in line_iterator.enumerate() {
        let mut start_time = SystemTime::now();

        /*
         * - Split sample line on whitespace
         * - Try to parse each data point to `f64`
         * - Convert each `f64` (floating-point) data point to a fixed-point `i64` with 8 bit precision
         * - Convert `i64` to little endian 8 byte array representation
         * - Flatten 8 byte array to 8 byte values
         * - Collect all the 8 byte values for each data point and add them to one array
         */
        let sample: Vec<u8> = sample_line?
            .split_whitespace()
            .filter_map(|data_point| data_point.parse::<f64>().ok())
            // 256 = 2^8 -> 8 bit fixed-point precision (shift left 8 bits)
            .flat_map(|data_point| ((data_point * 256f64).floor() as i64).to_le_bytes())
            .collect();

        // println!("Sample: {:02X?}", &sample);
        // println!("Sample array size: {}\n", &sample.len());

        let res: Response;

        // Time to read sample
        write!(
            bench_file,
            "{},",
            start_time
                .elapsed()
                .expect("error elapsed time")
                .as_micros()
        )?;
        start_time = SystemTime::now();

        // Encrypt on IoT device
        if !args.gateway {
            // Encrypt the sample
            let Ok(ct_sample) = protect(
                &client_id,
                &mut state,
                ProtectionAlgorithm::AesGcm128,
                &sample,
            ) else {
                panic!("Sample encryption error. Sample: {:02X?}", &sample);
            };

            // Time to encrypt sample
            write!(
                bench_file,
                "{},",
                start_time
                    .elapsed()
                    .expect("error elapsed time")
                    .as_micros()
            )?;
            start_time = SystemTime::now();

            // println!("C sample: {:02X?}", &ct_sample);

            res = http_client
                .post(&ingest_endpoint)
                .bearer_auth(auth_token.token().await)
                .json(&vec![IngestMetricEvent {
                    metric: "ecg_test::json".into(),
                    value: CipherTextValue { c: ct_sample },
                    source: Some("IoT Device Simulator".into()),
                }])
                .send()
                .await?;
        } else if args.gateway_authenticate {
            // Time to get here since reading sample (should be close to 0 since no encryption happens here)
            write!(
                bench_file,
                "{},",
                start_time
                    .elapsed()
                    .expect("error elapsed time")
                    .as_micros()
            )?;
            start_time = SystemTime::now();

            res = http_client
                .post(&ingest_endpoint)
                .json(&GatewayIngestMetricEvent {
                    timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
                    metric: "ecg_test::json".into(),
                    value: sample,
                    source: Some("IoT Device Simulator".into()),
                })
                .send()
                .await?;
        } else {
            // Time to get here since reading sample (should be close to 0 since no encryption happens here)
            write!(
                bench_file,
                "{},",
                start_time
                    .elapsed()
                    .expect("error elapsed time")
                    .as_micros()
            )?;
            start_time = SystemTime::now();

            res = http_client
                .post(&ingest_endpoint)
                .bearer_auth(auth_token.token().await)
                .json(&GatewayIngestMetricEvent {
                    timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
                    metric: "ecg_test::json".into(),
                    value: sample,
                    source: Some("IoT Device Simulator".into()),
                })
                .send()
                .await?;
        }

        // Time for ingestion
        writeln!(
            bench_file,
            "{}",
            start_time
                .elapsed()
                .expect("error elapsed time")
                .as_micros()
        )?;

        println!(
            "Sample {} ingested at {}: {}, via {}",
            i,
            res.headers()[DATE].to_str().unwrap(),
            res.status(),
            if args.gateway { "gateway" } else { "MOZAIK" }
        );

        if i + 1 >= args.count.try_into().unwrap() {
            break;
        }

        thread::sleep(time::Duration::from_millis(args.interval));
    }

    Ok(())
}
