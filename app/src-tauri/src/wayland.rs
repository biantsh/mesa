//! Apps can't grab global shortcuts on Wayland. Desktops with the XDG GlobalShortcuts portal (KDE Plasma, GNOME 48+,
//! Hyprland) bind one for us; elsewhere the user can add a custom shortcut that runs `mesa-app --search`.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU32, Ordering},
};

use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const SEARCH_ID: &str = "search";
const APP_ID: &str = "Mesa";

pub fn is_wayland() -> bool {
    let backend = std::env::var("GDK_BACKEND").unwrap_or_default();
    !backend.starts_with("x11") && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
}

pub fn bind_search(
    preferred: &str,
    mut bound: impl FnMut(Option<String>),
    mut pressed: impl FnMut(),
) -> Result<(), String> {
    let connection = Connection::session().map_err(|e| format!("no session bus ({e})"))?;
    register(&connection);
    let portal = Proxy::new(&connection, DESTINATION, PATH, INTERFACE).map_err(|e| e.to_string())?;
    portal
        .get_property::<u32>("version")
        .map_err(|e| format!("the desktop has no shortcuts portal ({e})"))?;
    let signals = portal.receive_all_signals().map_err(|e| e.to_string())?;

    let token = new_token();
    let session_token = new_token();
    let mut created = request(&connection, &token, || {
        let options = HashMap::from([
            ("handle_token", Value::from(token.as_str())),
            ("session_handle_token", Value::from(session_token.as_str())),
        ]);
        portal.call::<_, _, OwnedObjectPath>("CreateSession", &(options,))
    })?;
    let session = match created.remove("session_handle").as_deref() {
        Some(Value::Str(path)) => path.to_string(),
        Some(Value::ObjectPath(path)) => path.to_string(),
        _ => return Err("the portal made no session".into()),
    };
    let session = ObjectPath::try_from(session).map_err(|e| e.to_string())?;

    let shortcut = HashMap::from([
        ("description", Value::from("Open Mesa's search")),
        ("preferred_trigger", Value::from(preferred)),
    ]);
    let shortcuts = vec![(SEARCH_ID, shortcut)];
    let token = new_token();
    let mut results = request(&connection, &token, || {
        let options = HashMap::from([("handle_token", Value::from(token.as_str()))]);
        portal.call::<_, _, OwnedObjectPath>("BindShortcuts", &(&session, &shortcuts, "", options))
    })?;
    bound(results.remove("shortcuts").as_deref().and_then(trigger_of));

    for message in signals {
        match message.header().member().map(|member| member.as_str()) {
            Some("Activated") => {
                if let Ok((_, id, _, _)) = message
                    .body()
                    .deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>)>()
                    && id == SEARCH_ID
                {
                    pressed();
                }
            }
            Some("ShortcutsChanged") => {
                if let Ok((_, shortcuts)) = message
                    .body()
                    .deserialize::<(OwnedObjectPath, Vec<(String, HashMap<String, OwnedValue>)>)>()
                {
                    bound(trigger_in(&shortcuts));
                }
            }
            _ => {}
        }
    }
    Err("the desktop's shortcuts portal went away".into())
}

fn register(connection: &Connection) {
    if let Ok(registry) = Proxy::new(connection, DESTINATION, PATH, "org.freedesktop.host.portal.Registry") {
        let _ = registry.call::<_, _, ()>("Register", &(APP_ID, HashMap::<&str, Value>::new()));
    }
}

fn new_token() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!("mesa_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

fn request(
    connection: &Connection,
    token: &str,
    call: impl FnOnce() -> zbus::Result<OwnedObjectPath>,
) -> Result<HashMap<String, OwnedValue>, String> {
    let sender = connection
        .unique_name()
        .ok_or("no name on the session bus")?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("{PATH}/request/{sender}/{token}");
    let request = Proxy::new(connection, DESTINATION, path.as_str(), "org.freedesktop.portal.Request")
        .map_err(|e| e.to_string())?;
    let mut responses = request.receive_signal("Response").map_err(|e| e.to_string())?;
    call().map_err(|e| e.to_string())?;
    let message = responses.next().ok_or("the portal didn't answer")?;
    let (code, results) = message
        .body()
        .deserialize::<(u32, HashMap<String, OwnedValue>)>()
        .map_err(|e| e.to_string())?;
    match code {
        0 => Ok(results),
        1 => Err("it was turned down".into()),
        _ => Err("the portal couldn't do it".into()),
    }
}

fn trigger_in(shortcuts: &[(String, HashMap<String, OwnedValue>)]) -> Option<String> {
    let (_, about) = shortcuts.iter().find(|(id, _)| id == SEARCH_ID)?;
    match about.get("trigger_description").map(|value| &**value) {
        Some(Value::Str(text)) if !text.is_empty() => Some(text.to_string()),
        _ => None,
    }
}

fn trigger_of(shortcuts: &Value<'_>) -> Option<String> {
    let Value::Array(shortcuts) = shortcuts else {
        return None;
    };
    shortcuts.iter().find_map(|shortcut| {
        let Value::Structure(shortcut) = shortcut else {
            return None;
        };
        let (Some(Value::Str(id)), Some(Value::Dict(about))) = (shortcut.fields().first(), shortcut.fields().get(1))
        else {
            return None;
        };
        if id.as_str() != SEARCH_ID {
            return None;
        }
        about.iter().find_map(|(key, value)| {
            let value = if let Value::Value(inner) = value {
                &**inner
            } else {
                value
            };
            match (key, value) {
                (Value::Str(key), Value::Str(text)) if key.as_str() == "trigger_description" && !text.is_empty() => {
                    Some(text.to_string())
                }
                _ => None,
            }
        })
    })
}
