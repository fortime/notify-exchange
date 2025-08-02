use std::{
    pin::Pin,
    task::{Context, Poll},
};

use reqwest::Url;
use tokio::signal::unix::{self, Signal, SignalKind};

use crate::error::{self, NotifyExchangeResult, NotifyExchangeResultExt as _};

#[derive(Debug)]
pub struct Signals(Vec<(SignalKind, Signal)>);

impl Signals {
    /// Should be called inside tokio runtime
    pub fn new(signal_kinds: Vec<SignalKind>) -> NotifyExchangeResult<Self> {
        let mut signals = Vec::with_capacity(signal_kinds.len());
        for kind in signal_kinds {
            signals.push((
                kind,
                unix::signal(kind).whatever(format!("Unable to create signal: {kind:?}"))?,
            ));
        }
        Ok(Self(signals))
    }
}

impl Future for Signals {
    type Output = SignalKind;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        for (kind, signal) in self.0.iter_mut() {
            match signal.poll_recv(cx) {
                Poll::Pending => continue,
                Poll::Ready(_) => return Poll::Ready(*kind),
            }
        }
        Poll::Pending
    }
}

pub trait ToTuple {
    type Tuple;

    fn to_tuple(&self) -> Self::Tuple;
}

macro_rules! replace_item {
    ($_src:tt, $target:tt) => {
        $target
    };
}

macro_rules! count_item {
    ($_first: tt $(,)?) => {
        1usize
    };
    ($first: tt, $($item: tt),*) => {
        1 + count_item!($($item),*)
    };
}

macro_rules! impl_into_tuple {
    ($($index: tt),+ $(,)?) => {
        impl<T> ToTuple for [T; count_item!($($index),+)]
        where
            T: Copy,
        {
            type Tuple = ($(replace_item!($index, T)),+);

            fn to_tuple(&self) -> Self::Tuple {
                ($(self[$index]),+)
            }
        }
    };
}

impl_into_tuple!(0, 1);
impl_into_tuple!(0, 1, 2);
impl_into_tuple!(0, 1, 2, 3);

pub fn extract_params<'a, const N: usize>(
    params: &'a str,
    command_format: &str,
) -> NotifyExchangeResult<[&'a str; N]> {
    let params: Vec<&str> = params.splitn(N, '|').collect();
    params.try_into().map_err(|_| {
        error::invalid_request(format!(
            "Invalid command, it should be called like: {command_format}"
        ))
    })
}

pub fn extend_url(url: &mut Url, path: &str) -> NotifyExchangeResult<()> {
    url.path_segments_mut()
        .map(|mut p| {
            p.extend(path.split('/'));
        })
        .map_err(|_| error::internal_server_error("Invalid url, can't push path"))?;
    Ok(())
}
