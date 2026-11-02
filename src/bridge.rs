//! D-Bus bridge (pure-Rust `zbus`, no libdbus) + offline-testable formatting.
//!
//! The bridge is `Option` at the filesystem level: with a reachable bus it serves
//! real introspection/properties/methods; without one, stint-busfs runs in demo
//! mode (the same behaviour as the C prototype with no bus).

use crate::types::BusType;
use zbus::blocking::Connection;
use zvariant::Value;

/// A connected D-Bus session/system bus.
pub struct Bridge {
    conn: Connection,
}

impl Bridge {
    /// Connect to the bus; `None` when unreachable (demo mode).
    pub fn connect(bus: BusType) -> Option<Bridge> {
        let conn = match bus {
            BusType::Session => Connection::session(),
            BusType::System => Connection::system(),
        }
        .ok()?;
        Some(Bridge { conn })
    }

    /// Well-known names on the bus (unique `:1.x` names filtered out), sorted.
    pub fn list_services(&self) -> Vec<String> {
        let Ok(proxy) = zbus::blocking::Proxy::new(
            &self.conn,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        ) else {
            return vec![];
        };
        let Ok(names) = proxy.call::<_, _, Vec<String>>("ListNames", &()) else {
            return vec![];
        };
        let mut well_known: Vec<String> =
            names.into_iter().filter(|n| !n.starts_with(':')).collect();
        well_known.sort();
        well_known
    }

    /// `Introspect` at the service's object path.
    pub fn introspect(&self, service: &str, object_path: &str) -> Result<String, String> {
        let proxy = zbus::blocking::Proxy::new(
            &self.conn,
            service,
            object_path,
            "org.freedesktop.DBus.Introspectable",
        )
        .map_err(|e| e.to_string())?;
        proxy
            .call::<_, _, String>("Introspect", &())
            .map_err(|e| e.to_string())
    }

    /// First interface name in the introspection XML (used for GetAll).
    pub fn first_interface(xml: &str) -> Option<String> {
        xml_interfaces(xml).into_iter().next()
    }

    /// `Properties.GetAll` on the first discovered interface, formatted as
    /// `key = value` lines sorted by key.
    pub fn properties(&self, service: &str, object_path: &str) -> Result<String, String> {
        let xml = self
            .introspect(service, object_path)
            .map_err(|e| format!("introspect failed: {e}"))?;
        let iface = Self::first_interface(&xml)
            .ok_or_else(|| "no interfaces in introspection".to_string())?;
        let proxy = zbus::blocking::Proxy::new(
            &self.conn,
            service,
            object_path,
            "org.freedesktop.DBus.Properties",
        )
        .map_err(|e| e.to_string())?;
        let props = proxy
            .call::<_, _, std::collections::HashMap<String, zvariant::OwnedValue>>(
                "GetAll",
                &(iface),
            )
            .map_err(|e| e.to_string())?;
        let mut lines: Vec<(String, String)> = props
            .iter()
            .map(|(k, v)| (k.clone(), fmt_owned(v)))
            .collect();
        lines.sort();
        if lines.is_empty() {
            return Ok("(empty)\n".into());
        }
        let mut out = lines
            .into_iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        out.push('\n');
        Ok(out)
    }

    /// Call a method by name on the service (interface = the service name, as
    /// in the C prototype), with no arguments.
    pub fn call_method(
        &self,
        service: &str,
        object_path: &str,
        method: &str,
    ) -> Result<String, String> {
        let proxy = zbus::blocking::Proxy::new(&self.conn, service, object_path, service)
            .map_err(|e| e.to_string())?;
        let reply: zvariant::OwnedValue = proxy.call(method, &()).map_err(|e| e.to_string())?;
        Ok(fmt_owned(&reply))
    }
}

/// Format an owned D-Bus value for `cat`.
pub fn fmt_owned(v: &zvariant::OwnedValue) -> String {
    match Value::try_from(v) {
        Ok(val) => fmt_value(&val),
        Err(_) => "(?)".into(),
    }
}

/// Recursive D-Bus value formatter (dicts, arrays, structs, variants, scalars).
pub fn fmt_value(v: &Value<'_>) -> String {
    fmt_value_depth(v, 0)
}

fn fmt_value_depth(v: &Value<'_>, depth: usize) -> String {
    if depth > 32 {
        return "(…)".into();
    }
    match v {
        Value::Str(s) => s.as_str().to_string(),
        Value::ObjectPath(p) => p.as_str().to_string(),
        Value::Signature(s) => s.as_str().to_string(),
        Value::Bool(b) => b.to_string(),
        Value::U8(b) => format!("0x{b:02x}"),
        Value::U16(n) => n.to_string(),
        Value::I16(n) => n.to_string(),
        Value::U32(n) => n.to_string(),
        Value::I32(n) => n.to_string(),
        Value::U64(n) => n.to_string(),
        Value::I64(n) => n.to_string(),
        Value::F64(n) => n.to_string(),
        Value::Value(inner) => format!("<{}>", fmt_value_depth(inner, depth + 1)),
        Value::Array(a) => {
            let items: Vec<String> = a.iter().map(|e| fmt_value_depth(e, depth + 1)).collect();
            format!("[{}]", items.join(" "))
        }
        Value::Structure(s) => {
            let fields: Vec<String> = s
                .fields()
                .iter()
                .map(|f| fmt_value_depth(f, depth + 1))
                .collect();
            format!("({})", fields.join(" "))
        }
        Value::Dict(d) => {
            let pairs: Vec<String> = d
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{{{} {}}}",
                        fmt_value_depth(k, depth + 1),
                        fmt_value_depth(v, depth + 1)
                    )
                })
                .collect();
            pairs.join(" ")
        }
        _ => "(?)".into(),
    }
}

/// Interface names in introspection XML (`<interface name="…">`), in order.
pub fn xml_interfaces(xml: &str) -> Vec<String> {
    xml_tag_attr(xml, "interface", "name")
}

/// Signal names in introspection XML (`<signal name="…">`), in order.
pub fn xml_signals(xml: &str) -> Vec<String> {
    xml_tag_attr(xml, "signal", "name")
}

fn xml_tag_attr(xml: &str, tag: &str, attr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    let open = format!("<{tag}");
    while let Some(pos) = rest.find(&open) {
        let after = &rest[pos + open.len()..];
        let end = after.find('>').unwrap_or(after.len());
        let elem = &after[..end];
        if let Some(v) = attr_value(elem, attr) {
            out.push(v);
        }
        rest = &after[end.min(after.len())..];
    }
    out
}

fn attr_value(elem: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=");
    let pos = elem.find(&needle)?;
    let rest = &elem[pos + needle.len()..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &rest[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"
<node>
  <interface name="org.freedesktop.UPower.Device">
    <method name="Refresh"><arg name="a" type="s" direction="in"/></method>
    <signal name="Changed">
      <arg name="state" type="u"/>
    </signal>
    <signal name="另行信号"/>
    <property name="Type" type="u" access="read"/>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable"/>
</node>"#;

    #[test]
    fn xml_extraction() {
        let ifaces = xml_interfaces(XML);
        assert_eq!(
            ifaces,
            vec![
                "org.freedesktop.UPower.Device".to_string(),
                "org.freedesktop.DBus.Introspectable".to_string()
            ]
        );
        assert_eq!(
            xml_signals(XML),
            vec!["Changed".to_string(), "另行信号".to_string()]
        );
        assert_eq!(
            Bridge::first_interface(XML).as_deref(),
            Some("org.freedesktop.UPower.Device")
        );
    }

    #[test]
    fn xml_extraction_tolerates_garbage() {
        assert_eq!(xml_tag_attr("", "interface", "name"), Vec::<String>::new());
        assert_eq!(
            xml_tag_attr("<interface name=broken>", "interface", "name"),
            Vec::<String>::new()
        );
        assert_eq!(
            xml_tag_attr("<interface><noattr>", "interface", "name"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn value_formatting() {
        assert_eq!(fmt_value(&Value::Bool(true)), "true");
        assert_eq!(fmt_value(&Value::U32(7)), "7");
        assert_eq!(fmt_value(&Value::U8(0xff)), "0xff");
        assert_eq!(fmt_value(&Value::Str("hello".into())), "hello");
        let mut arr = zvariant::Array::new(zvariant::Signature::from_str_unchecked("i"));
        arr.append(Value::I32(1)).unwrap();
        arr.append(Value::I32(2)).unwrap();
        assert_eq!(fmt_value(&Value::Array(arr)), "[1 2]");
        let st = zvariant::StructureBuilder::new()
            .add_field(false)
            .add_field(9_u64)
            .build();
        assert_eq!(fmt_value(&Value::Structure(st)), "(false 9)");
    }
}
