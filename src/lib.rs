use std::ffi::CString;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::io::AsRawFd;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PtyError {
    #[error("Failed to open PTY: {0}")]
    OpenError(std::io::Error),
    #[error("Failed to fork process: {0}")]
    ForkError(std::io::Error),
    #[error("Read/Write failed: {0}")]
    IoError(#[from] std::io::Error),
}

pub struct Pty {
    master: OwnedFd,
    child_pid: libc::pid_t,
}

impl Pty {
    pub fn spawn(shell_path: &str, args: &[&str], rows: u16, cols: u16) -> Result<Self, PtyError> {
        unsafe {
            let master_fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            if master_fd < 0 {
                return Err(PtyError::OpenError(std::io::Error::last_os_error()));
            }
            let master = OwnedFd::from_raw_fd(master_fd);

            if libc::grantpt(master_fd) < 0 || libc::unlockpt(master_fd) < 0 {
                return Err(PtyError::OpenError(std::io::Error::last_os_error()));
            }

            let slave_name_ptr = libc::ptsname(master_fd);
            if slave_name_ptr.is_null() {
                return Err(PtyError::OpenError(std::io::Error::last_os_error()));
            }
            let slave_name = std::ffi::CStr::from_ptr(slave_name_ptr).to_owned();

            let mut ws: libc::winsize = std::mem::zeroed();
            ws.ws_row = rows;
            ws.ws_col = cols;
            
            let slave_fd = libc::open(slave_name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
            if slave_fd < 0 {
                return Err(PtyError::OpenError(std::io::Error::last_os_error()));
            }
            libc::ioctl(slave_fd, libc::TIOCSWINSZ, &ws);

            let pid = libc::fork();
            if pid < 0 {
                return Err(PtyError::ForkError(std::io::Error::last_os_error()));
            }

            if pid == 0 {
                libc::setsid();
                libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);

                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);

                if slave_fd > 2 { libc::close(slave_fd); }
                libc::close(master_fd);

                let path = CString::new(shell_path).unwrap();
                let mut argv: Vec<CString> = Vec::with_capacity(args.len() + 1);
                argv.push(path.clone());
                for arg in args {
                    argv.push(CString::new(*arg).unwrap());
                }
                let mut argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|s| s.as_ptr()).collect();
                argv_ptrs.push(std::ptr::null());

                let term = CString::new("TERM=xterm-256color").unwrap();
                let envp = [term.as_ptr(), std::ptr::null()];

                libc::execve(path.as_ptr(), argv_ptrs.as_ptr(), envp.as_ptr());
                libc::_exit(127);
            }

            libc::close(slave_fd);

            Ok(Pty {
                master,
                child_pid: pid,
            })
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = unsafe { libc::read(self.master.as_raw_fd(), buf.as_mut_ptr() as *mut _, buf.len()) };
        if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    pub fn write(&self, data: &[u8]) -> std::io::Result<usize> {
        let n = unsafe { libc::write(self.master.as_raw_fd(), data.as_ptr() as *const _, data.len()) };
        if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    pub fn resize(&self, rows: u16, cols: u16) -> std::io::Result<()> {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        ws.ws_row = rows;
        ws.ws_col = cols;
        let res = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
        if res < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
    }
}
