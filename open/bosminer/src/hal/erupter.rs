pub mod config;
pub mod device;
pub mod error;
pub mod icarus;

use ii_logging::macros::*;

use crate::work;
use error::ErrorKind;

use tokio_threadpool::blocking;

// use old futures which is compatible with current tokio
use futures::lock::Mutex;
use futures_01::future::poll_fn;

use failure::ResultExt;
use std::sync::Arc;

fn main_task(
    work_solver: work::Solver,
    mining_stats: Arc<Mutex<super::MiningStats>>,
    _shutdown: crate::hal::ShutdownSender,
) -> crate::error::Result<()> {
    info!("Block Erupter: finding device in USB...");
    let usb_context =
        libusb::Context::new().with_context(|_| ErrorKind::Usb("cannot create USB context"))?;
    let mut device = device::BlockErupter::find(&usb_context)
        .ok_or_else(|| ErrorKind::Usb("cannot find Block Erupter device"))?;

    info!("Block Erupter: initialization...");
    device.init()?;
    info!("Block Erupter: initialized and ready to solve the work!");

    let (generator, solution_sender) = work_solver.split();
    let mut solver = device.into_solver(generator);

    // iterate until there exists any work or the error occurs
    for solution in &mut solver {
        solution_sender.send(solution);

        ii_async_compat::block_on(mining_stats.lock()).unique_solutions += 1;
    }

    // check solver for errors
    solver.get_stop_reason()?;
    Ok(())
}

/// Entry point for running the hardware backend
pub fn run(
    work_solver: work::Solver,
    mining_stats: Arc<Mutex<super::MiningStats>>,
    shutdown: crate::hal::ShutdownSender,
) {
    // wrap `main_task` parameters to Option to overcome FnOnce closure inside FnMut
    let mut args = Some((work_solver, mining_stats, shutdown));

    // spawn future in blocking context which guarantees that the task is run in separate thread
    tokio::spawn(
        // Because `blocking` returns `Poll`, it is intended to be used from the context of
        // a `Future` implementation. Since we don't have a complicated requirement, we can use
        // `poll_fn` in this case.
        poll_fn(move || {
            blocking(|| {
                let (work_solver, mining_stats, shutdown) = args
                    .take()
                    .expect("`tokio_threadpool::blocking` called FnOnce more than once");
                if let Err(e) = main_task(work_solver, mining_stats, shutdown) {
                    error!("{}", e);
                }
            })
            .map_err(|_| panic!("the threadpool shut down"))
        }),
    );
}
