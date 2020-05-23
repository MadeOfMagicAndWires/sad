use super::errors::*;
use super::types::Task;
use async_std::sync::{channel, Receiver, Sender};
use futures::future::{try_join, try_join4};
use std::process::Stdio;
use tokio::{
  io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
  process::Command,
  task,
};

#[derive(Clone)]
pub struct SubprocessCommand {
  pub program: String,
  pub arguments: Vec<String>,
}

impl SubprocessCommand {
  pub fn stream(&self, stream: Receiver<SadResult<String>>) -> (Task, Receiver<SadResult<String>>) {
    let (tx, rx) = channel::<SadResult<String>>(1);
    let to = Sender::clone(&tx);
    let te = Sender::clone(&tx);
    let tt = Sender::clone(&tx);
    let ta = Sender::clone(&tx);

    let subprocess = Command::new(&self.program)
      .args(&self.arguments)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn();

    let mut child = match subprocess.into_sadness() {
      Ok(child) => child,
      Err(err) => {
        let handle = task::spawn(async move { tx.send(Err(err)).await });
        return (handle, rx);
      }
    };

    let mut stdin = match child.stdin.take() {
      Some(stdin) => BufWriter::new(stdin),
      None => {
        let err = Err(Failure::Pager("Invalid stin".into()));
        let handle = task::spawn(async move { tx.send(err).await });
        return (handle, rx);
      }
    };

    let mut stdout = match child.stdout.take() {
      Some(stdout) => BufReader::new(stdout),
      None => {
        let err = Err(Failure::Pager("Invalid stdout".into()));
        let handle = task::spawn(async move { tx.send(err).await });
        return (handle, rx);
      }
    };

    let mut stderr = match child.stderr.take() {
      Some(stderr) => BufReader::new(stderr),
      None => {
        let err = Err(Failure::Pager("Invalid stderr".into()));
        let handle = task::spawn(async move { tx.send(err).await });
        return (handle, rx);
      }
    };

    let handle_in = task::spawn(async move {
      while let Some(print) = stream.recv().await {
        match print {
          Ok(val) => {
            if let Err(err) = stdin.write(val.as_bytes()).await.into_sadness() {
              tx.send(Err(err)).await;
            }
          }
          Err(err) => tx.send(Err(err)).await,
        }
      }
      if let Err(err) = stdin.shutdown().await {
        tx.send(Err(err.into())).await;
      }
    });

    let handle_out = task::spawn(async move {
      loop {
        let mut buf = String::new();
        match stdout.read_line(&mut buf).await.into_sadness() {
          Ok(0) => return,
          Ok(_) => {
            to.send(Ok(buf)).await;
          }
          Err(err) => to.send(Err(err)).await,
        }
      }
    });

    let handle_err = task::spawn(async move {
      let mut buf = String::new();
      match stderr.read_to_string(&mut buf).await.into_sadness() {
        Err(err) => {
          te.send(Err(err)).await;
        }
        Ok(_) => {
          if !buf.is_empty() {
            te.send(Err(Failure::Pager(buf))).await
          }
        }
      }
    });

    let handle_child = task::spawn(async move {
      if let Err(err) = child.await {
        tt.send(Err(err.into())).await;
      }
    });

    let handle = task::spawn(async move {
      if let Err(err) = try_join4(handle_child, handle_in, handle_out, handle_err).await {
        ta.send(Err(err.into())).await;
      }
    });

    (handle, rx)
  }

  pub fn stream_connected(
    &self,
    stream: Receiver<SadResult<String>>,
  ) -> (Task, Receiver<SadResult<()>>) {
    let (tx, rx) = channel::<SadResult<()>>(1);
    let tt = Sender::clone(&tx);
    let ta = Sender::clone(&tx);

    let subprocess = Command::new(&self.program)
      .args(&self.arguments)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn();

    let mut child = match subprocess.into_sadness() {
      Ok(child) => child,
      Err(err) => {
        let handle = task::spawn(async move { tx.send(Err(err)).await });
        return (handle, rx);
      }
    };

    let mut stdin = match child.stdin.take() {
      Some(stdin) => BufWriter::new(stdin),
      None => {
        let err = Err(Failure::Pager("Invalid stin".into()));
        let handle = task::spawn(async move { tx.send(err).await });
        return (handle, rx);
      }
    };

    let handle_in = task::spawn(async move {
      while let Some(print) = stream.recv().await {
        match print {
          Ok(val) => {
            if let Err(err) = stdin.write(val.as_bytes()).await.into_sadness() {
              tx.send(Err(err)).await;
            }
          }
          Err(err) => tx.send(Err(err)).await,
        }
      }
      if let Err(err) = stdin.shutdown().await {
        tx.send(Err(err.into())).await;
      }
    });

    let handle_child = task::spawn(async move {
      if let Err(err) = child.await {
        tt.send(Err(err.into())).await;
      }
    });

    let handle = task::spawn(async move {
      if let Err(err) = try_join(handle_child, handle_in).await {
        ta.send(Err(err.into())).await;
      }
    });

    (handle, rx)
  }
}
