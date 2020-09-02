// Unless explicitly stated otherwise all files in this repository are licensed under the
// MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2020 Datadog, Inc.
//
use futures::prelude::*;
use futures::task::{Context, Poll, Waker};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{Error, ErrorKind, Result};
use std::pin::Pin;
use std::rc::Rc;

struct Waiter {
    units: u64,
    woken: bool,
    waker: Option<Waker>,
}

impl Future for Waiter {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.woken {
            return Poll::Ready(());
        }
        self.waker = Some(cx.waker().clone());
        return Poll::Pending;
    }
}

#[derive(Debug)]
struct State {
    avail: u64,
    list: VecDeque<*mut Waiter>,
    closed: bool,
}

impl State {
    fn new(avail: u64) -> Self {
        State {
            avail,
            list: VecDeque::new(),
            closed: false,
        }
    }

    fn available(&self) -> u64 {
        self.avail
    }

    fn queue(&mut self, units: u64) -> Box<Waiter> {
        // FIXME: I should pin this
        let mut waiter = Box::new(Waiter::new(units));
        self.list.push_back(waiter.as_mut());
        waiter
    }

    fn try_acquire(&mut self, units: u64) -> Result<bool> {
        if self.closed == true {
            return Err(Error::new(ErrorKind::BrokenPipe, "Semaphore Broken"));
        }

        if self.list.is_empty() && self.avail >= units {
            self.avail -= units;
            return Ok(true);
        }
        return Ok(false);
    }

    fn close(&mut self) {
        self.closed = true;
        loop {
            let cont = match self.list.pop_front() {
                None => None,
                Some(waitref) => {
                    let waiter = unsafe { &mut *waitref };
                    Some(waiter.wake())
                }
            };
            if let None = cont {
                break;
            }
        }
    }

    fn signal(&mut self, units: u64) -> Option<*mut Waiter> {
        self.avail += units;

        if let Some(waitref) = self.list.front() {
            let waiter = *waitref;
            let w = unsafe { &mut *waiter };
            if w.units <= self.avail {
                self.list.pop_front();
                return Some(waiter);
            }
        }
        None
    }
}

impl Waiter {
    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            self.woken = true;
            waker.wake();
        }
    }

    fn new(units: u64) -> Waiter {
        Waiter {
            units,
            woken: false,
            waker: None,
        }
    }
}

/// The permit is A RAII-friendly way to acquire semaphore resources.
///
/// Resources are held while the Permit is alive, and released when the
/// permit is dropped.
#[derive(Debug)]
pub struct Permit {
    units: u64,
    sem: Rc<RefCell<State>>,
}

impl Permit {
    fn new(units: u64, sem: Rc<RefCell<State>>) -> Permit {
        Permit {
            units,
            sem: sem.clone(),
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let waker = self.sem.borrow_mut().signal(self.units);
        waker.and_then(|w| {
            let waiter = unsafe { &mut *w };
            Some(waiter.wake())
        });
    }
}

/// An implementation of semaphore that doesn't use helper threads,
/// condition variables, and is friendly to single-threaded execution.
#[derive(Debug)]
pub struct Semaphore {
    state: Rc<RefCell<State>>,
}

impl Semaphore {
    /// Creates a new semaphore with the specified amount of units
    pub fn new(avail: u64) -> Semaphore {
        Semaphore {
            state: Rc::new(RefCell::new(State::new(avail))),
        }
    }

    /// Returns the amount of units currently available in this semaphore
    pub fn available(&self) -> u64 {
        self.state.borrow().available()
    }

    /// Blocks until a permit can be acquired with the specified amount of units.
    ///
    /// Returns Err() if the semaphore is closed during the wait.
    pub async fn acquire_permit(&self, units: u64) -> Result<Permit> {
        self.acquire(units).await?;
        Ok(Permit::new(units, self.state.clone()))
    }

    /// Acquires the specified amount of units from this semaphore.
    ///
    /// The caller is then responsible to release it. Whenever possible,
    /// prefer acquire_permit().
    pub async fn acquire(&self, units: u64) -> Result<()> {
        loop {
            let mut state = self.state.borrow_mut();
            if state.try_acquire(units)? {
                return Ok(());
            }

            let waiter = state.queue(units);
            drop(state);
            waiter.await;
        }
    }

    /// Signals the semaphore to release the specified amount of units.
    ///
    /// This needs to be paired with a call to acquire(). You should not
    /// call this if the units were acquired with acquire_permit().
    pub fn signal(&self, units: u64) {
        let waker = self.state.borrow_mut().signal(units);
        waker.and_then(|w| {
            let waiter = unsafe { &mut *w };
            Some(waiter.wake())
        });
    }

    /// Closes the semaphore
    ///
    /// All existing waiters will return Err(), and no new waiters are allowed.
    pub fn close(&self) {
        let mut state = self.state.borrow_mut();
        state.close();
    }
}
