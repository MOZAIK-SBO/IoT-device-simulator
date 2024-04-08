use std::{
    env,
    error::Error,
    fs::File,
    io::{BufRead, BufReader},
    thread, time,
};

use client_auth::AuthToken;
use dotenv::dotenv;
use libmozaik_iot::{protect, DeviceState, ProtectionAlgorithm};

use reqwest::header::DATE;
use types::IngestMetricEvent;

use crate::types::CipherTextValue;

pub mod types;

/*
dataset_description.txt

Description for the ecg_dataset.txt file containing samples for ML inference for the heartbeat use-case of MOZAIK:

The first line of the file contains an integer X representing the number of samples in the file.
The second line of the file contains an integer Y representing the vector length of the samples.
From the third line onwards the samples are listed, separated by a line break (each sample is written in a new line). Hence, the file contains X samples of length Y, each in a new line.
 - each sample corresponds to a vector of Y numbers in floating point representation, separated by a space (" ").

Conversion to representation needed for computing in MPC:
In MPC we work in integer arithmetic, with fixed-point representation. For now we will fix the fixed-point precision to 16 bits, and the integer part will be represented by 16 bits as well.
To convert number x to the desired format, compute f(x) = floor(x . 2^16). In other words, the input is multiplied by 2^f, where f denotes the fixed-point precision. Afterwards, a floor function is applied, a floor function in this case means rounding down to the nearest integer (= cutting the decimal part off).
Further, the resulting 32-bit integer is encoded in 4 bytes, starting with the least significant byte.

8-bit precision:
In MPC we work in integer arithmetic, with fixed-point representation. For now we will fix the fixed-point precision to 8 bits, and the integer part will be represented by 8 bits as well.
To convert number x to the desired format, compute f(x) = floor(x . 2^8). In other words, the input is multiplied by 2^f, where f denotes the fixed-point precision. Afterwards, a floor function is applied, a floor function in this case means rounding down to the nearest integer (= cutting the decimal part off).
Further, the resulting 16-bit integer is encoded in 2 bytes, starting with the least significant byte.
*/

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Env
    dotenv().ok();
    let ingest_endpoint = env::var("INGEST_ENDPOINT").unwrap();

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

    // Iterate over each sample in the dataset
    for (i, sample_line) in line_iterator.enumerate() {
        /*
         * - Split sample line on whitespace
         * - Try to parse each data point to `f64`
         * - Convert each `f64` (floating-point) data point to a fixed-point `i16` with 8 bit precision
         * - Convert `i16` to little endian 2 byte array representation
         * - Flatten 2 byte array to 2 byte values
         * - Collect all the 2 byte values for each data point and add them to one array
         */
        let sample: Vec<u8> = sample_line?
            .split_whitespace()
            .filter_map(|data_point| data_point.parse::<f64>().ok())
            // 65536 = 2^16 -> 16 bit fixed-point precision (shift left 16 bits)
            // 256 = 2^8 -> 8 bit fixed-point precision (shift left 8 bits)
            .flat_map(|data_point| ((data_point * 256f64).floor() as i16).to_le_bytes())
            .collect();

        // println!("Sample: {:02X?}", &sample);
        // println!("Sample array size: {}\n", &sample.len());

        // Encrypt the sample
        let Ok(ct_sample) = protect(
            &client_id,
            &mut state,
            ProtectionAlgorithm::AesGcm128,
            &sample,
        ) else {
            panic!("Sample encryption error. Sample: {:02X?}", &sample);
        };

        // println!("C sample: {:02X?}", &ct_sample);

        let res = http_client
            .post(&ingest_endpoint)
            .bearer_auth(auth_token.token().await)
            .json(&vec![IngestMetricEvent {
                metric: "ecg_test::json".into(),
                value: CipherTextValue { c: ct_sample },
                source: Some("IoT Device Simulator".into()),
            }])
            .send()
            .await?;

        println!(
            "Sample {} ingested at {}: {}",
            i,
            res.headers()[DATE].to_str().unwrap(),
            res.status()
        );

        thread::sleep(time::Duration::from_millis(1000));
    }

    Ok(())
}
