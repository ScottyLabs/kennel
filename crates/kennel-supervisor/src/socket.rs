use std::net::SocketAddr;
use std::os::unix::io::{AsFd, AsRawFd, RawFd};

use crate::config::{ListenKind, ListenSpec};

pub struct BoundSocket {
    pub fd: RawFd,
    pub address: Option<SocketAddr>,
    // Held to keep the FD alive. Dropped when the process stops.
    _socket: socket2::Socket,
}

/// Bind all sockets specified in the listen config. Clears CLOEXEC on
/// each FD so the child process inherits them after exec.
pub fn bind_sockets(specs: &[ListenSpec]) -> crate::Result<Vec<BoundSocket>> {
    let mut bound = Vec::new();

    for spec in specs {
        let socket = match spec.kind {
            ListenKind::Tcp => {
                let addr = spec.address.as_deref().unwrap_or("127.0.0.1:0");
                let socket = socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::STREAM,
                    Some(socket2::Protocol::TCP),
                )?;
                socket.set_reuse_address(true)?;
                let parsed: SocketAddr = addr.parse().map_err(|e| {
                    crate::SupervisorError::SocketBind(format!("invalid address {addr}: {e}"))
                })?;
                socket.bind(&parsed.into())?;
                socket.listen(spec.backlog as i32)?;
                socket
            }
            ListenKind::UnixStream => {
                let path = spec.path.as_ref().ok_or_else(|| {
                    crate::SupervisorError::SocketBind("unix listener requires path".into())
                })?;
                let socket =
                    socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
                socket.bind(&socket2::SockAddr::unix(path)?)?;
                socket.listen(spec.backlog as i32)?;
                socket
            }
        };

        clear_cloexec(&socket)?;

        let fd = socket.as_raw_fd();
        let local_addr = socket.local_addr()?.as_socket();

        bound.push(BoundSocket {
            fd,
            address: local_addr,
            _socket: socket,
        });
    }

    Ok(bound)
}

/// socket2 sets CLOEXEC by default. Clear it so the FD survives exec
/// into the child process.
fn clear_cloexec(socket: &socket2::Socket) -> crate::Result<()> {
    use nix::fcntl::{FcntlArg, FdFlag, fcntl};
    let borrowed = socket.as_fd();
    let flags = fcntl(borrowed, FcntlArg::F_GETFD)
        .map_err(|e| crate::SupervisorError::SocketBind(format!("fcntl F_GETFD: {e}")))?;
    let mut fd_flags = FdFlag::from_bits_truncate(flags);
    fd_flags.remove(FdFlag::FD_CLOEXEC);
    fcntl(borrowed, FcntlArg::F_SETFD(fd_flags))
        .map_err(|e| crate::SupervisorError::SocketBind(format!("fcntl F_SETFD: {e}")))?;
    Ok(())
}

/// SD_LISTEN_FDS_START per the systemd socket activation protocol.
pub const LISTEN_FDS_START: RawFd = 3;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ListenSpec;

    #[test]
    fn bind_tcp_socket() {
        let specs = vec![ListenSpec {
            name: "http".into(),
            kind: ListenKind::Tcp,
            address: Some("127.0.0.1:0".into()),
            path: None,
            backlog: 128,
        }];

        let bound = bind_sockets(&specs).unwrap();
        assert_eq!(bound.len(), 1);
        assert!(bound[0].fd >= 0);

        let addr = bound[0].address.unwrap();
        assert_ne!(addr.port(), 0);
    }

    #[test]
    fn bind_multiple_tcp_sockets() {
        let specs = vec![
            ListenSpec {
                name: "http".into(),
                kind: ListenKind::Tcp,
                address: Some("127.0.0.1:0".into()),
                path: None,
                backlog: 128,
            },
            ListenSpec {
                name: "grpc".into(),
                kind: ListenKind::Tcp,
                address: Some("127.0.0.1:0".into()),
                path: None,
                backlog: 64,
            },
        ];

        let bound = bind_sockets(&specs).unwrap();
        assert_eq!(bound.len(), 2);

        let port1 = bound[0].address.unwrap().port();
        let port2 = bound[1].address.unwrap().port();
        assert_ne!(port1, port2);
    }

    #[test]
    fn bind_unix_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");

        let specs = vec![ListenSpec {
            name: "control".into(),
            kind: ListenKind::UnixStream,
            address: None,
            path: Some(sock_path.clone()),
            backlog: 128,
        }];

        let bound = bind_sockets(&specs).unwrap();
        assert_eq!(bound.len(), 1);
        assert!(bound[0].fd >= 0);
        assert!(sock_path.exists());
    }

    #[test]
    fn bind_unix_without_path_fails() {
        let specs = vec![ListenSpec {
            name: "bad".into(),
            kind: ListenKind::UnixStream,
            address: None,
            path: None,
            backlog: 128,
        }];

        let result = bind_sockets(&specs);
        assert!(result.is_err());
    }

    #[test]
    fn empty_specs_returns_empty() {
        let bound = bind_sockets(&[]).unwrap();
        assert!(bound.is_empty());
    }

    #[test]
    fn cloexec_cleared() {
        use nix::fcntl::{FcntlArg, FdFlag, fcntl};
        use std::os::unix::io::BorrowedFd;

        let specs = vec![ListenSpec {
            name: "http".into(),
            kind: ListenKind::Tcp,
            address: Some("127.0.0.1:0".into()),
            path: None,
            backlog: 128,
        }];

        let bound = bind_sockets(&specs).unwrap();
        // SAFETY: The fd is valid because bind_sockets just created and
        // returned it, and bound[0]._socket keeps the fd alive.
        let borrowed = unsafe { BorrowedFd::borrow_raw(bound[0].fd) };
        let flags = fcntl(borrowed, FcntlArg::F_GETFD).unwrap();
        let fd_flags = FdFlag::from_bits_truncate(flags);
        assert!(!fd_flags.contains(FdFlag::FD_CLOEXEC));
    }
}
