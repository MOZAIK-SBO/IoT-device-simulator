use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    thread, time,
};

use libmozaik_iot::{protect, DeviceState, ProtectionAlgorithm};

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

fn main() -> io::Result<()> {
    let nonce = [
        0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0xff, 0xff, 0xff, 0xff,
    ]; // this should be a fresh nonce
    let key = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10,
    ]; // this should be a fresh device key

    let mut state = DeviceState::new(nonce, key);

    const USER_ID: &str = "e7514b7a-9293-4c83-b733-a53e0e449635";

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

    // Iterate over each sample in the dataset
    for sample in line_iterator {
        // Convert each `f64` data point from the sample to a fixed-point with 16 bit precision `i32`, which is then encoded in a little endian 4 byte array
        let sample_data_points: Vec<[u8; 4]> = sample?
            .split_whitespace()
            .filter_map(|data_point| data_point.parse::<f64>().ok())
            // 65536.0 = 2^16 -> 16 bit fixed-point precision
            .map(|data_point| ((data_point * 65536.0).floor() as i32).to_le_bytes())
            .collect();

        println!("{:02X?}\n", &sample_data_points);

        // Encrypt each data point from this sample
        let ct_sample_data_points: Vec<Vec<u8>> = sample_data_points
            .iter()
            .filter_map(|data_point| {
                protect(
                    USER_ID,
                    &mut state,
                    ProtectionAlgorithm::AesGcm128,
                    data_point,
                )
                .ok()
            })
            .collect();

        println!("{:02X?}", &ct_sample_data_points);

        thread::sleep(time::Duration::from_secs(15));
    }

    Ok(())
}
