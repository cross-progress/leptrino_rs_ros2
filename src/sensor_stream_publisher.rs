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

    let publisher = node.create_publisher::<geometry_msgs::msg::Wrench>("raw")?;

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

    while context.ok() {
        if let Ok((n, data)) = interface.read() {
            buffer.push_slice(data.into_iter().take(n).collect::<Vec<u8>>().as_slice());
            if let Ok(v) = find_streamed_data(&mut buffer) {
                let f: Vec<f64> = v
                    .iter()
                    .zip(limits.iter())
                    .map(|(a, b)| a.to_owned() as f64 * b.to_owned() / 10000.0)
                    .collect();

                let msg = Wrench {
                    force: Vector3 {
                        x: f[0],
                        y: f[1],
                        z: f[2],
                    },
                    torque: Vector3 {
                        x: f[3],
                        y: f[4],
                        z: f[5],
                    },
                };

                publisher.publish(msg)?;
            }
            std::thread::sleep(Duration::from_micros(10));
        }
    }
    Ok(())
}
