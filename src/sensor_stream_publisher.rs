use anyhow::Result;
use geometry_msgs::msg::{Vector3, Wrench};
use leptrino_rs::buffer::*;
use leptrino_rs::interface::Interface;
use rclrs::*;
use ringbuf::{producer::Producer, rb::LocalRb, storage::Heap};
use std::time::Duration;

fn main() -> Result<(), rclrs::RclrsError> {
    let context = Context::default_from_env()?;
    let executor = context.create_basic_executor();

    let node = executor.create_node("leptrino")?;

    let use_calibration = true;

    let publisher = if use_calibration {
        node.create_publisher::<geometry_msgs::msg::Wrench>("calibrated")?
    } else {
        node.create_publisher::<geometry_msgs::msg::Wrench>("raw")?
    };

    const BUF_SIZE: usize = 2048;
    const IP_ADDR: &str = "192.168.100.211";
    const READ_TIMEOUT: Duration = Duration::from_millis(1);
    let mut interface = Interface::new(IP_ADDR, Some(READ_TIMEOUT)).unwrap();

    interface.stop_streaming().unwrap();
    std::thread::sleep(Duration::from_secs(1));

    // Clear kernel buffer
    loop {
        let (n, _) = interface.read().unwrap();
        if n < 1024 {
            break;
        }
    }

    let mut buffer = LocalRb::<Heap<u8>>::new(BUF_SIZE);

    interface.request_sensor_limits().unwrap();
    std::thread::sleep(Duration::from_secs(1));

    let (n, data) = interface.read().unwrap();
    buffer.push_slice(data.into_iter().take(n).collect::<Vec<u8>>().as_slice());
    let limits = find_sensor_limits(&mut buffer).unwrap();

    // Start sensor data streaming
    interface.start_streaming().unwrap();
    std::thread::sleep(Duration::from_secs(1));

    let calibration = if use_calibration {
        let mut calibration_data = [0f64; 6];
        if let Ok((n, data)) = interface.read() {
            loop {
                buffer.push_slice(data.into_iter().take(n).collect::<Vec<u8>>().as_slice());
                let mut counter = 0;
                const NUM_CALIBRATION_DATA: isize = 10;
                while let Ok(v) = find_streamed_data(&mut buffer) {
                    let f: Vec<f64> = v
                        .iter()
                        .zip(limits.iter())
                        .map(|(a, b)| a.to_owned() as f64 * b.to_owned() / 10000.0)
                        .collect();

                    for i in 0..6 {
                        calibration_data[i] += f[i] / (NUM_CALIBRATION_DATA as f64);
                    }

                    counter += 1;
                    // println!("{}: {:?}", counter, calibration_data);
                    if counter >= NUM_CALIBRATION_DATA {
                        break;
                    }
                }
                if counter >= NUM_CALIBRATION_DATA {
                    break;
                }
            }
        }
        calibration_data
    } else {
        [0f64; 6]
    };

    while context.ok() {
        if let Ok((n, data)) = interface.read() {
            buffer.push_slice(data.into_iter().take(n).collect::<Vec<u8>>().as_slice());
            while let Ok(v) = find_streamed_data(&mut buffer) {
                let raw: Vec<f64> = v
                    .iter()
                    .zip(limits.iter())
                    .map(|(a, b)| a.to_owned() as f64 * b.to_owned() / 10000.0)
                    .collect();

                let calibrated: Vec<f64> = if use_calibration {
                    raw.iter()
                        .zip(calibration.iter())
                        .map(|(a, b)| a.to_owned() - b.to_owned())
                        .map(|a| if a.abs() < 1.0 { 0.0 } else { a })
                        .collect()
                } else {
                    raw
                };

                let msg = Wrench {
                    force: Vector3 {
                        x: calibrated[0],
                        y: calibrated[1],
                        z: calibrated[2],
                    },
                    torque: Vector3 {
                        x: calibrated[3],
                        y: calibrated[4],
                        z: calibrated[5],
                    },
                };

                publisher.publish(msg)?;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}
