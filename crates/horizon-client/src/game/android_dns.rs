use jni::{
    JNIEnv, JavaVM,
    objects::{JObject, JString, JValue},
};
use std::net::{IpAddr, SocketAddr};

pub(super) fn install() {
    horizon_protocol::set_name_servers_source(name_servers);
}

fn name_servers() -> Vec<SocketAddr> {
    let Some(app) = super::ANDROID_APP.get() else {
        return Vec::new();
    };
    let result = (|| -> jni::errors::Result<Vec<SocketAddr>> {
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast())? };
        let mut env = vm.attach_current_thread()?;
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let result = active_network_dns(&mut env, &activity);
        if result.is_err() {
            let _ = env.exception_clear();
        }
        result
    })();
    match result {
        Ok(servers) => servers,
        Err(e) => {
            log::warn!("Horizon: the system's DNS servers: {e}");
            Vec::new()
        }
    }
}

fn active_network_dns(
    env: &mut JNIEnv,
    activity: &JObject,
) -> jni::errors::Result<Vec<SocketAddr>> {
    let service = env.new_string("connectivity")?;
    let manager = env
        .call_method(
            activity,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&service)],
        )?
        .l()?;
    if manager.is_null() {
        return Ok(Vec::new());
    }
    let network = env
        .call_method(&manager, "getActiveNetwork", "()Landroid/net/Network;", &[])?
        .l()?;
    if network.is_null() {
        return Ok(Vec::new());
    }
    let properties = env
        .call_method(
            &manager,
            "getLinkProperties",
            "(Landroid/net/Network;)Landroid/net/LinkProperties;",
            &[JValue::Object(&network)],
        )?
        .l()?;
    if properties.is_null() {
        return Ok(Vec::new());
    }
    let list = env
        .call_method(&properties, "getDnsServers", "()Ljava/util/List;", &[])?
        .l()?;
    let count = env.call_method(&list, "size", "()I", &[])?.i()?;
    let mut servers = Vec::new();
    for index in 0..count {
        let address = env
            .call_method(&list, "get", "(I)Ljava/lang/Object;", &[JValue::Int(index)])?
            .l()?;
        let text = env
            .call_method(&address, "getHostAddress", "()Ljava/lang/String;", &[])?
            .l()?;
        let text: String = env.get_string(&JString::from(text))?.into();
        if let Some(server) = parse_server(&text)
            && !servers.contains(&server)
        {
            servers.push(server);
        }
    }
    Ok(servers)
}

fn parse_server(text: &str) -> Option<SocketAddr> {
    let ip: IpAddr = text.split('%').next()?.parse().ok()?;
    let usable = match ip {
        IpAddr::V4(v4) => !v4.is_unspecified(),
        IpAddr::V6(v6) => !v6.is_unspecified() && !v6.is_unicast_link_local(),
    };
    usable.then(|| SocketAddr::new(ip, 53))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_properties_addresses_become_name_servers() {
        assert_eq!(
            parse_server("192.168.1.1"),
            Some("192.168.1.1:53".parse().unwrap())
        );
        assert_eq!(
            parse_server("2001:4860:4860::8888%wlan0"),
            Some("[2001:4860:4860::8888]:53".parse().unwrap())
        );
        assert_eq!(parse_server("fe80::1%wlan0"), None);
        assert_eq!(parse_server("0.0.0.0"), None);
        assert_eq!(parse_server("not an address"), None);
    }
}
