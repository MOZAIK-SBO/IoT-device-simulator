use std::{
    env,
    fs::File,
    io::{self, BufRead, BufReader},
    thread, time,
};

use dotenv::dotenv;
use libmozaik_iot::{protect, DeviceState, ProtectionAlgorithm};

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
In MPC we work in integer arithmetic, with fixed-point representation. For now we will fix the fixed-point precision to 16 bits, and the integer part will be represented by 16 bits as well.
To convert number x to the desired format, compute f(x) = floor(x . 2^16). In other words, the input is multiplied by 2^f, where f denotes the fixed-point precision. Afterwards, a floor function is applied, a floor function in this case means rounding down to the nearest integer (= cutting the decimal part off).
Further, the resulting 32-bit integer is encoded in 4 bytes, starting with the least significant byte.
*/

#[tokio::main]
async fn main() -> io::Result<()> {
    dotenv().ok();

    // TODO: nonce + key
    let nonce = [
        0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0xff, 0xff, 0xff, 0xff,
    ]; // this should be a fresh nonce
    let key = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10,
    ]; // this should be a fresh device key

    let mut state = DeviceState::new(nonce, key);

    let user_id: String = env::var("USER_ID").unwrap();

    let dataset = File::open("../ecg_dataset.txt")?;
    let dataset_buff_reader = BufReader::new(dataset);

    let mut line_iterator = dataset_buff_reader.lines();

    let _x_samples = if let Some(Ok(x)) = line_iterator.next() {
        println!("Amount of samples: {}.", &x);
        x
    } else {
        panic!("Cannot read amount of samples.");
    };

    let _y_sample_length = if let Some(Ok(y)) = line_iterator.next() {
        println!("Sample length: {}.", &y);
        y
    } else {
        panic!("Cannot read sample length.");
    };

    let http_client = reqwest::Client::new();
    let mozaik_obelisk_endpoint = env::var("MOZAIK_OBELISK_ENDPOINT").unwrap();

    // Iterate over each sample in the dataset
    for sample_line in line_iterator {
        /*
         * - Split sample line on whitespace
         * - Try to parse each data point to `f64`
         * - Convert each `f64` (floating-point) data point to a fixed-point `i32` with 16 bit precision
         * - Convert `i32` to little endian 4 byte array representation
         * - Flatten 4 byte array to 4 byte values
         */
        let sample: Vec<u8> = sample_line?
            .split_whitespace()
            .filter_map(|data_point| data_point.parse::<f64>().ok())
            // 65536.0 = 2^16 -> 16 bit fixed-point precision
            .flat_map(|data_point| ((data_point * 65536.0).floor() as i32).to_le_bytes())
            .collect();

        println!("Sample: {:02X?}\n", &sample);

        // Encrypt the sample
        let Ok(ct_sample) = protect(
            &user_id,
            &mut state,
            ProtectionAlgorithm::AesGcm128,
            &sample,
        ) else {
            panic!("Sample encryption error. Sample: {:02X?}", &sample);
        };

        println!("CT sample: {:02X?}\n\n", &ct_sample);

        let res = http_client
            .post(format!(
                "{mozaik_obelisk_endpoint}/obelisk/ingest/datasetId"
            ))
            .json(&vec![IngestMetricEvent {
                timestamp: None,
                metric: "ECG".into(),
                value: ct_sample,
                source: None,
                tags: None,
                location: None,
                elevation: None,
            }])
            .send()
            .await;

        println!("{:#?}", res);

        thread::sleep(time::Duration::from_secs(15));
    }

    Ok(())
}
