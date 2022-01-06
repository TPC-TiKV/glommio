// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2020 Datadog, Inc.
//
use glommio::{
    prelude::*,

};
use async_channel::{Sender, Receiver, self};
use std::io::Result;

async fn server(receiver: Receiver<usize>) -> Result<()> {
    loop {
        // while let Ok(i) = receiver.try_recv(){
            // println!("recv {}", i);
        // }
        println!("async recv");
        let i = receiver.recv().await.unwrap();
        println!("async recv done {}", i);
    }
    // Ok(())
}

fn main() -> Result<()> {
    let (tx, rx) = async_channel::unbounded();
    // Skip CPU0 because that is commonly used to host interrupts. That depends on
    // system configuration and most modern systems will balance it, but that it is
    // still common enough that it is worth excluding it in this benchmark
    for _ in 0..1 {
        let rx = rx.clone();
        let builder = LocalExecutorBuilder::new(Placement::Unbound);
        builder.name("server").spawn(|| async move {
            // If you try `top` during the execution of the first batch, you
            // will see that the CPUs should not be at 100%. A single connection will
            // not be enough to extract all the performance available in the cores.
            server(rx).await.unwrap()
            // This should drive the CPU utilization to 100%.
            // Asynchronous execution needs parallelism to thrive!
        })?;
    }
    drop(rx);

    let mut i = 0;
    loop {
        tx.try_send(i).unwrap();
        if i % 1000 == 0 {
            println!("send {}", i);
        }
        i += 1;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Congrats for getting to the end of this example!
    //
    // Now can you adapt it, so it uses multiple executors and all CPUs in your
    // system?
    // server_handle.join().unwrap();
    // Ok(())
}
