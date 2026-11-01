//! Shared loopback server for generated sites.
use crate::{cli, document};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const ADDRESS: &str = "127.0.0.1:8765";
const SERVICE: &str = "cmrak-preview-v2";
const RELOAD: &str = r#"<script data-cmrak-preview>(()=>{let etag=__ETAG__;const events=new EventSource('/_preview/events?path='+encodeURIComponent(location.pathname)+'&revision='+encodeURIComponent(__REVISION__));events.onmessage=e=>{events.close();const fragment=JSON.parse(e.data);if(fragment!==null)location.hash=fragment;location.reload()};async function check(){try{let r=await fetch(location.href,{method:'HEAD',cache:'no-store'});let next=r.headers.get('etag');if(etag&&next&&etag!==next){location.reload();return}if(next)etag=next}catch{}}check();setInterval(check,750)})();</script>"#;

fn shared_root() -> PathBuf {
    std::env::temp_dir().join("cmrak-preview")
}

fn hash(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn preview_relative_url(root: &Path, page: &Path) -> Result<String, String> {
    let root = dunce::canonicalize(root).map_err(|e| e.to_string())?;
    let page = dunce::canonicalize(page).map_err(|e| e.to_string())?;
    page.strip_prefix(&root)
        .map_err(|_| "preview page is outside preview root")?;
    let base = url::Url::from_directory_path(&root).map_err(|_| "invalid preview root")?;
    let file = url::Url::from_file_path(&page).map_err(|_| "invalid preview page")?;
    base.make_relative(&file)
        .ok_or_else(|| "invalid preview URL".to_owned())
}

fn notification_path(root: &Path, page: &Path) -> PathBuf {
    root.join(".events")
        .join(hash(page.to_string_lossy().as_bytes()))
}

fn notify(root: &Path, page: &Path, fragment: Option<&str>) -> Result<(), String> {
    let dir = root.join(".events");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let value = serde_json::json!({"revision": format!("{nonce}-{}", std::process::id()), "fragment": fragment});
    let mut temp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    temp.write_all(value.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    temp.persist(notification_path(root, page))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn notification(root: &Path, page: &Path) -> serde_json::Value {
    fs::read(notification_path(root, page))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_else(|| serde_json::json!({"revision": "", "fragment": null}))
}

fn project_name(root: &Path) -> String {
    let path = root.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let path = path.to_lowercase();
    hash(path.as_bytes())[..16].to_owned()
}

fn request(method: &str, path: &str) -> Result<String, String> {
    let address: SocketAddr = ADDRESS.parse().unwrap();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(300))
        .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {ADDRESS}\r\nX-Preview-Control: stop\r\nConnection: close\r\n\r\n").map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    Ok(response)
}

fn healthy(root: &Path) -> bool {
    let Ok(response) = request("GET", "/_preview/health") else {
        return false;
    };
    let Some((header, body)) = response.split_once("\r\n\r\n") else {
        return false;
    };
    if !header.starts_with("HTTP/1.1 200 ") {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    value["service"] == SERVICE
        && value["root"]
            .as_str()
            .is_some_and(|path| dunce::canonicalize(path).ok() == dunce::canonicalize(root).ok())
}

fn ensure_server(root: &Path) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    if healthy(root) {
        return Ok(());
    }
    if TcpStream::connect_timeout(&ADDRESS.parse().unwrap(), Duration::from_millis(200)).is_err() {
        #[cfg(not(windows))]
        {
            use std::process::{Command, Stdio};
            let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
            child
                .arg("__site_preview_server")
                .arg(root)
                .arg("8765")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            child
                .spawn()
                .map_err(|e| format!("cannot start preview server: {e}"))?;
        }
        #[cfg(windows)]
        spawn_windows_server(root)?;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if healthy(root) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "preview service unavailable or {ADDRESS} belongs to another service"
    ))
}

#[cfg(windows)]
fn spawn_windows_server(root: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION,
            STARTUPINFOW,
        },
    };

    // Quote an argument according to the Windows command-line parsing rules.
    fn quote(value: &std::ffi::OsStr, output: &mut Vec<u16>) {
        output.push(b'"' as u16);
        let mut slashes = 0;
        for c in value.encode_wide() {
            if c == b'\\' as u16 {
                slashes += 1;
            } else {
                let count = if c == b'"' as u16 {
                    slashes * 2 + 1
                } else {
                    slashes
                };
                output.extend(std::iter::repeat_n(b'\\' as u16, count));
                output.push(c);
                slashes = 0;
            }
        }
        output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        output.push(b'"' as u16);
    }

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Vec::new();
    for arg in [
        exe.as_os_str(),
        std::ffi::OsStr::new("__site_preview_server"),
        root.as_os_str(),
        std::ffi::OsStr::new("8765"),
    ] {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        quote(arg, &mut command);
    }
    command.push(0);
    let application: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // Disabling handle inheritance is essential: callers may have inheritable
    // pipe handles other than the three standard handles.
    let result = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut process,
        )
    };
    if result == 0 {
        return Err(format!(
            "cannot start preview server: {}",
            std::io::Error::last_os_error()
        ));
    }
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    Ok(())
}

/// Compile to the shared preview site and print a reusable URL.
pub fn compile(values: Vec<String>) -> Result<(), String> {
    let mut arguments = vec!["compile".to_owned()];
    let mut fragment = None;
    let mut values = values.into_iter();
    while let Some(value) = values.next() {
        if value == "--fragment" {
            fragment = Some(values.next().ok_or("--fragment requires a value")?);
        } else if let Some(value) = value.strip_prefix("--fragment=") {
            fragment = Some(value.to_owned());
        } else {
            arguments.push(value);
        }
    }
    let Some(original) = cli::parse(arguments.clone())? else {
        return Ok(());
    };
    if original.site_root.is_some()
        || original.output != original.input.with_extension("html")
        || original.standalone && arguments.iter().any(|arg| arg == "--standalone")
    {
        return Err(
            "serve INPUT manages its site output; omit output, --site-root and --standalone".into(),
        );
    }
    let root = dunce::canonicalize(original.root.as_deref().unwrap_or(Path::new(".")))
        .map_err(|e| e.to_string())?;
    let input = dunce::canonicalize(&original.input).map_err(|e| e.to_string())?;
    let relative = input
        .strip_prefix(&root)
        .map_err(|_| "input must be inside --root")?;
    let site = shared_root().join(project_name(&root)).join("md");
    arguments.push("--site-root".into());
    arguments.push(site.to_string_lossy().into_owned());
    let args = cli::parse(arguments)?.ok_or("no input")?;
    let locks = shared_root().join(".locks");
    fs::create_dir_all(&locks).map_err(|e| e.to_string())?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(locks.join(format!("{}.lock", hash(input.to_string_lossy().as_bytes()))))
        .map_err(|e| e.to_string())?;
    lock.lock()
        .map_err(|e| format!("cannot lock compilation: {e}"))?;
    document::compile(&args).map_err(|errors| {
        errors
            .into_iter()
            .map(|d| {
                format!(
                    "{}:{}:{}: error: {}",
                    args.input.display(),
                    d.line,
                    d.column,
                    d.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    ensure_server(&shared_root())?;
    let page = crate::site::page_path(&site.join(relative));
    let page = dunce::canonicalize(page).map_err(|e| e.to_string())?;
    notify(&shared_root(), &page, fragment.as_deref())?;
    let relative = preview_relative_url(&shared_root(), &page)?;
    println!("http://{ADDRESS}/{relative}");
    Ok(())
}

pub fn stop() -> Result<(), String> {
    if !healthy(&shared_root()) {
        return Err(format!("preview service is not running on {ADDRESS}"));
    }
    let response = request("POST", "/_preview/shutdown")?;
    if response.starts_with("HTTP/1.1 200 ") {
        Ok(())
    } else {
        Err("preview service refused shutdown".into())
    }
}

pub fn serve(root: &Path, port: u16) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let root = dunce::canonicalize(root).map_err(|e| e.to_string())?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| e.to_string())?;
    eprintln!(
        "Preview: http://{}/\nDirectory: {}",
        listener.local_addr().map_err(|e| e.to_string())?,
        root.display()
    );
    for stream in listener.incoming() {
        let stream = stream.map_err(|e| e.to_string())?;
        let root = root.clone();
        std::thread::spawn(move || {
            let _ = respond(stream, &root);
        });
    }
    Ok(())
}

fn safe_file(root: &Path, target: &str) -> Option<PathBuf> {
    let url = url::Url::parse("http://localhost/")
        .ok()?
        .join(target)
        .ok()?;
    let base = url::Url::from_directory_path(root).ok()?;
    let path = url.path().strip_prefix('/')?;
    let mut file = base.join(path).ok()?.to_file_path().ok()?;
    if file.is_dir() {
        file.push("index.html");
    }
    let file = dunce::canonicalize(file).ok()?;
    (file.starts_with(root) && file.is_file()).then_some(file)
}

fn respond(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut data = [0; 16384];
    let mut length = 0;
    while length < data.len() && !data[..length].windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut data[length..])?;
        if n == 0 {
            break;
        }
        length += n;
    }
    let request = String::from_utf8_lossy(&data[..length]);
    let mut first = request.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("");
    let target = first.next().unwrap_or("");
    let clean = target.split('?').next().unwrap_or("");
    if clean == "/_preview/shutdown" {
        let permitted = method == "POST"
            && !request
                .lines()
                .any(|line| line.to_ascii_lowercase().starts_with("origin:"))
            && request
                .lines()
                .any(|line| line.eq_ignore_ascii_case("x-preview-control: stop"));
        if permitted {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
            std::process::exit(0);
        }
        stream.write_all(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return Ok(());
    }
    if method != "GET" && method != "HEAD" {
        stream.write_all(
            b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return Ok(());
    }
    if clean == "/_preview/health" {
        let body = serde_json::json!({"service": SERVICE, "root": root}).to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
            body.len(),
            if method == "HEAD" { "" } else { &body }
        )?;
        return Ok(());
    }
    if clean == "/_preview/events" && method == "GET" {
        let parsed = url::Url::parse("http://localhost/")
            .unwrap()
            .join(target)
            .ok();
        let params: std::collections::HashMap<_, _> = parsed
            .as_ref()
            .map(|url| url.query_pairs().into_owned().collect())
            .unwrap_or_default();
        let Some(file) = params.get("path").and_then(|path| safe_file(root, path)) else {
            stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            return Ok(());
        };
        let revision = params.get("revision").map(String::as_str).unwrap_or("");
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n: connected\n\n")?;
        loop {
            let value = notification(root, &file);
            if value["revision"].as_str().unwrap_or("") != revision {
                write!(stream, "data: {}\n\n", value["fragment"])?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
            stream.write_all(b": keepalive\n\n")?;
        }
    }
    let Some(file) = safe_file(root, target) else {
        stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return Ok(());
    };
    if !clean.ends_with('/')
        && !clean.ends_with("/index.html")
        && file.file_name().is_some_and(|name| name == "index.html")
    {
        let query = target
            .split_once('?')
            .map_or(String::new(), |(_, query)| format!("?{query}"));
        write!(
            stream,
            "HTTP/1.1 301 Moved Permanently\r\nLocation: {clean}/{query}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )?;
        return Ok(());
    }
    let mut body = fs::read(&file)?;
    let etag = hash(&body);
    let html = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
    if html {
        let index = body
            .windows(7)
            .rposition(|b| b.eq_ignore_ascii_case(b"</body>"))
            .unwrap_or(body.len());
        let revision = serde_json::to_string(&format!("\"{etag}\"")).unwrap();
        let event_revision =
            serde_json::to_string(notification(root, &file)["revision"].as_str().unwrap_or(""))
                .unwrap();
        let script = RELOAD
            .replace("__ETAG__", &revision)
            .replace("__REVISION__", &event_revision);
        body.splice(index..index, script.bytes());
    }
    let mime = mime_guess::from_path(&file).first_or_octet_stream();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nETag: \"{etag}\"\r\nCache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if method == "GET" {
        stream.write_all(&body)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_routes_stay_under_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("nested/index.html"), "hello").unwrap();
        assert_eq!(
            safe_file(&root, "/nested/?v=1"),
            Some(root.join("nested/index.html"))
        );
        assert!(safe_file(&root, "/%2e%2e/etc/passwd").is_none());
    }
    #[test]
    fn notifications_keep_fragment_and_advance_for_unchanged_pages() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("index.html");
        notify(dir.path(), &page, Some("#章节")).unwrap();
        let first = notification(dir.path(), &page);
        assert_eq!(first["fragment"], "#章节");
        notify(dir.path(), &page, None).unwrap();
        let second = notification(dir.path(), &page);
        assert_ne!(first["revision"], second["revision"]);
        assert!(second["fragment"].is_null());
    }
    #[test]
    fn preview_url_is_encoded_and_rejects_outside_pages() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("preview");
        let page = root.join("空 格").join("index.html");
        fs::create_dir_all(page.parent().unwrap()).unwrap();
        fs::write(&page, "test").unwrap();
        assert_eq!(
            preview_relative_url(&root, &page).unwrap(),
            "%E7%A9%BA%20%E6%A0%BC/index.html"
        );
        let outside = dir.path().join("outside.html");
        fs::write(&outside, "test").unwrap();
        assert!(preview_relative_url(&root, &outside).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn preview_url_normalizes_root_alias() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("preview");
        fs::create_dir(&root).unwrap();
        let page = root.join("index.html");
        fs::write(&page, "test").unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        assert_eq!(preview_relative_url(&alias, &page).unwrap(), "index.html");
    }
    #[test]
    fn project_ids_are_stable_and_distinct() {
        assert_eq!(project_name(Path::new("/a")), project_name(Path::new("/a")));
        assert_ne!(project_name(Path::new("/a")), project_name(Path::new("/b")));
    }
}
