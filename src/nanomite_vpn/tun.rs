//! TUN/TAP virtual network interface.
//! Cross-platform: real TUN device on Linux, stub elsewhere.

use crate::nanomite_vpn::VpnError;
use async_trait::async_trait;
use std::io;

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::*;
    use tokio_tun::Tun as TokioTun;

    pub struct LinuxTun {
        tun: TokioTun,
    }

    impl LinuxTun {
        pub async fn new(name: &str, ip: &str, netmask: &str) -> Result<Self, VpnError> {
            let tun = tokio_tun::TunBuilder::new()
                .name(name)
                .tap(false)
                .try_build()
                .map_err(|e| VpnError::TunCreate(e.to_string()))?;

            let _ = std::process::Command::new("ip")
                .args(["link", "set", name, "up"])
                .status();
            let _ = std::process::Command::new("ip")
                .args([
                    "addr",
                    "add",
                    &format!("{}/{}", ip, netmask_prefix(netmask)),
                    "dev",
                    name,
                ])
                .status();

            Ok(Self { tun })
        }
    }

    #[async_trait]
    impl TunReadWrite for LinuxTun {
        async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.tun
                .recv(buf)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }

        async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.tun
                .send(buf)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }
    }

    fn netmask_prefix(netmask: &str) -> u32 {
        match netmask {
            "255.255.255.0" => 24,
            "255.255.0.0" => 16,
            "255.0.0.0" => 8,
            _ => 24,
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod stub_impl {
    use super::*;

    pub struct StubTun {
        // Kept for future introspection/logging of the stub iface.
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        opened: bool,
    }

    impl StubTun {
        pub fn new(name: &str, _ip: &str, _netmask: &str) -> Result<Self, VpnError> {
            Ok(Self {
                name: name.to_string(),
                opened: true,
            })
        }
    }

    #[async_trait]
    impl TunReadWrite for StubTun {
        async fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Ok(0)
        }

        async fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Ok(_buf.len())
        }
    }
}

/// A virtual TUN device for sending/receiving IP packets.
pub struct TunDevice {
    #[cfg(target_os = "linux")]
    inner: linux_impl::LinuxTun,
    #[cfg(not(target_os = "linux"))]
    inner: stub_impl::StubTun,
}

#[async_trait]
pub trait TunReadWrite: Send + Sync {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    async fn write(&mut self, buf: &[u8]) -> io::Result<usize>;
}

impl TunDevice {
    pub async fn new(name: &str, ip: &str, netmask: &str) -> Result<Self, VpnError> {
        #[cfg(target_os = "linux")]
        {
            let inner = linux_impl::LinuxTun::new(name, ip, netmask).await?;
            Ok(Self { inner })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let inner = stub_impl::StubTun::new(name, ip, netmask)?;
            Ok(Self { inner })
        }
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(target_os = "linux")]
        {
            self.inner.read(buf).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.inner.read(buf).await
        }
    }

    pub async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(target_os = "linux")]
        {
            self.inner.write(buf).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.inner.write(buf).await
        }
    }
}

/// Platform-specific configuration details for the TUN device.
#[derive(Debug, Clone)]
pub struct TunConfig {
    pub name: String,
    pub ip: String,
    pub netmask: String,
}

impl Default for TunConfig {
    fn default() -> Self {
        Self {
            name: "nanomite0".into(),
            ip: "10.200.0.1".into(),
            netmask: "255.255.255.0".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tun_config_default() {
        let cfg = TunConfig::default();
        assert_eq!(cfg.name, "nanomite0");
        assert_eq!(cfg.ip, "10.200.0.1");
    }
}
