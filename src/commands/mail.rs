//! Durable, socket-scoped mailboxes with best-effort one-line pane notifications.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use serde::Serialize;

use crate::cli::{MailArgs, MailLsArgs, MailReadArgs, MailSendArgs, ReplyArgs};
use crate::commands::{code, die, family, io::deliver_paste, no_such_session, Ctx};
use crate::output::print_json;
use crate::paths::{create_private_dir_all, encode_state_component, Paths};
use crate::session;
use crate::store::Store;

const RESERVED_VERBS: &[&str] = &["ls", "read", "send"];
const ARCHIVE_TIMESTAMP: &str = ".exited_at";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mailbox {
    Session(String),
    Pane(String),
    Local,
}

impl Mailbox {
    fn address(&self) -> String {
        match self {
            Self::Session(name) => name.clone(),
            Self::Pane(pane) => format!("pane:{pane}"),
            Self::Local => "local".to_string(),
        }
    }

    fn dir(&self, root: &Path) -> PathBuf {
        match self {
            Self::Session(name) => root.join(session_component(name)),
            Self::Pane(pane) => root.join("panes").join(encode_state_component(pane)),
            Self::Local => root.join("panes").join("local"),
        }
    }
}

fn session_component(name: &str) -> String {
    let encoded = encode_state_component(name);
    if encoded.is_empty() || matches!(encoded.as_str(), "." | ".." | "panes") {
        format!("%00{encoded}")
    } else {
        encoded
    }
}

fn absolute_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

#[derive(Debug, Clone)]
struct Recipient {
    mailbox: Mailbox,
    ping_pane: String,
}

impl Recipient {
    fn address(&self) -> String {
        self.mailbox.address()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct Message {
    from: String,
    to: String,
    date: String,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    in_reply_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
    body: String,
}

impl Message {
    fn render(&self) -> String {
        let mut rendered = format!(
            "From: {}\nTo: {}\nDate: {}\nId: {}\n",
            safe_header(&self.from),
            safe_header(&self.to),
            safe_header(&self.date),
            safe_header(&self.id)
        );
        if let Some(reply) = &self.in_reply_to {
            rendered.push_str(&format!("In-Reply-To: {}\n", safe_header(reply)));
        }
        if let Some(subject) = &self.subject {
            rendered.push_str(&format!("Subject: {}\n", safe_header(subject)));
        }
        rendered.push('\n');
        rendered.push_str(&self.body);
        rendered
    }

    fn parse(raw: &str) -> Result<Self> {
        let (headers, body) = raw
            .split_once("\n\n")
            .context("mail message is missing its header separator")?;
        let mut from = None;
        let mut to = None;
        let mut date = None;
        let mut id = None;
        let mut in_reply_to = None;
        let mut subject = None;
        for line in headers.lines() {
            let Some((name, value)) = line.split_once(':') else {
                bail!("invalid mail header: {line}");
            };
            let value = value.trim().to_string();
            match name {
                "From" => from = Some(value),
                "To" => to = Some(value),
                "Date" => date = Some(value),
                "Id" => id = Some(value),
                "In-Reply-To" => in_reply_to = Some(value),
                "Subject" => subject = Some(value),
                _ => {}
            }
        }
        Ok(Self {
            from: from.context("mail message is missing From")?,
            to: to.context("mail message is missing To")?,
            date: date.context("mail message is missing Date")?,
            id: id.context("mail message is missing Id")?,
            in_reply_to,
            subject,
            body: body.to_string(),
        })
    }
}

fn safe_header(value: &str) -> String {
    value
        .split(['\r', '\n'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0) as libc::time_t;
    let mut utc = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: gmtime_r initializes `utc` when it returns a non-null pointer.
    let result = unsafe { libc::gmtime_r(&seconds, utc.as_mut_ptr()) };
    if result.is_null() {
        return "1970-01-01T00:00:00Z".to_string();
    }
    // SAFETY: the non-null return above guarantees initialization.
    let utc = unsafe { utc.assume_init() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        utc.tm_year + 1900,
        utc.tm_mon + 1,
        utc.tm_mday,
        utc.tm_hour,
        utc.tm_min,
        utc.tm_sec
    )
}

#[derive(Debug, Clone, Copy)]
enum Folder {
    Inbox,
    Sent,
}

impl Folder {
    fn name(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MailStore {
    root: PathBuf,
    archive_root: PathBuf,
}

impl MailStore {
    pub(crate) fn new(paths: &Paths, socket: Option<&str>) -> Self {
        Self {
            root: absolute_path(paths.socket_state_dir("mail", socket)),
            archive_root: absolute_path(Store::new(paths, socket).mail_archive_dir()),
        }
    }

    fn validate_send_root(&self) -> Result<()> {
        let display = self.root.to_string_lossy();
        if display.chars().any(char::is_control) {
            bail!("mail state path contains a control character");
        }
        Ok(())
    }

    fn mailbox_dir(&self, mailbox: &Mailbox) -> PathBuf {
        mailbox.dir(&self.root)
    }

    fn ensure_mailbox(&self, mailbox: &Mailbox) -> Result<PathBuf> {
        let dir = self.mailbox_dir(mailbox);
        create_private_dir_all(&dir.join("inbox").join(".read"))?;
        create_private_dir_all(&dir.join("sent"))?;
        Ok(dir)
    }

    fn next_id(&self, mailbox: &Mailbox) -> Result<String> {
        let dir = self.ensure_mailbox(mailbox)?;
        let seq_path = dir.join("seq");
        let mut seq = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(&seq_path)
            .with_context(|| format!("opening {}", seq_path.display()))?;
        let lock_result = unsafe { libc::flock(seq.as_raw_fd(), libc::LOCK_EX) };
        if lock_result != 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("locking {}", seq_path.display()));
        }

        let result: Result<String> = (|| {
            let mut raw = String::new();
            seq.read_to_string(&mut raw)?;
            let mut value = raw.trim().parse::<u64>().unwrap_or(0);
            loop {
                value = value.checked_add(1).context("mail id counter overflowed")?;
                let id = format!("m{value}");
                let inbox = dir.join("inbox").join(format!("{id}.md"));
                let sent = dir.join("sent").join(format!("{id}.md"));
                if !inbox.exists() && !sent.exists() {
                    seq.seek(SeekFrom::Start(0))?;
                    seq.set_len(0)?;
                    writeln!(seq, "{value}")?;
                    seq.sync_data()?;
                    return Ok(id);
                }
            }
        })();

        let unlock_result = unsafe { libc::flock(seq.as_raw_fd(), libc::LOCK_UN) };
        if unlock_result != 0 && result.is_ok() {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("unlocking {}", seq_path.display()));
        }
        result.with_context(|| format!("allocating mail id in {}", dir.display()))
    }

    fn write_copy(
        &self,
        mailbox: &Mailbox,
        folder: Folder,
        mut message: Message,
    ) -> Result<(PathBuf, Message)> {
        let dir = self.ensure_mailbox(mailbox)?;
        let local_id = self.next_id(mailbox)?;
        message.id = format!("{local_id}@{}", mailbox.address());
        let path = dir.join(folder.name()).join(format!("{local_id}.md"));
        atomic_write_new(&path, message.render().as_bytes())?;
        Ok((path, message))
    }

    fn write_dual(
        &self,
        sender: &Mailbox,
        recipient: &Mailbox,
        message: Message,
    ) -> Result<(PathBuf, Message)> {
        let (sent_path, _) = self.write_copy(sender, Folder::Sent, message.clone())?;
        match self.write_copy(recipient, Folder::Inbox, message) {
            Ok(delivered) => Ok(delivered),
            Err(delivery_err) => match std::fs::remove_file(&sent_path) {
                Ok(()) => Err(delivery_err
                    .context("recipient write failed; rolled back the sender's sent copy")),
                Err(rollback_err) => Err(anyhow!(
                    "recipient write failed: {delivery_err:#}; rolling back {} also failed: \
                     {rollback_err}",
                    sent_path.display()
                )),
            },
        }
    }

    fn inbox_path(&self, mailbox: &Mailbox, id: &str) -> Option<PathBuf> {
        let id = local_id(id)?;
        Some(
            self.mailbox_dir(mailbox)
                .join("inbox")
                .join(format!("{id}.md")),
        )
    }

    fn read_inbox(&self, mailbox: &Mailbox, id: &str) -> Result<(PathBuf, String, Message)> {
        let Some(path) = self.inbox_path(mailbox, id) else {
            bail!("invalid mail id");
        };
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let message =
            Message::parse(&raw).with_context(|| format!("parsing {}", path.display()))?;
        Ok((path, raw, message))
    }

    fn mark_read(&self, mailbox: &Mailbox, id: &str) -> Result<()> {
        let id = local_id(id).context("invalid mail id")?;
        let read_dir = self.mailbox_dir(mailbox).join("inbox").join(".read");
        create_private_dir_all(&read_dir)?;
        let marker = read_dir.join(id);
        File::create(&marker).with_context(|| format!("creating {}", marker.display()))?;
        Ok(())
    }

    fn is_unread(&self, mailbox: &Mailbox, id: &str) -> bool {
        !self
            .mailbox_dir(mailbox)
            .join("inbox")
            .join(".read")
            .join(id)
            .exists()
    }

    pub(crate) fn unread_count_session(&self, name: &str) -> Result<usize> {
        self.unread_count(&Mailbox::Session(name.to_string()))
    }

    fn unread_count(&self, mailbox: &Mailbox) -> Result<usize> {
        let inbox = self.mailbox_dir(mailbox).join("inbox");
        let entries = match std::fs::read_dir(&inbox) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(err) => {
                return Err(err).with_context(|| format!("reading {}", inbox.display()));
            }
        };
        let mut unread = 0;
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", inbox.display()))?;
            let path = entry.path();
            if path.extension() != Some(OsStr::new("md")) {
                continue;
            }
            let Some(id) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            if self.is_unread(mailbox, id) {
                unread += 1;
            }
        }
        Ok(unread)
    }

    pub(crate) fn reset_session(&self, name: &str) -> Result<()> {
        let mailbox = Mailbox::Session(name.to_string());
        let dir = self.mailbox_dir(&mailbox);
        remove_dir_if_present(&dir)?;
        self.ensure_mailbox(&mailbox)?;
        std::fs::write(dir.join("seq"), b"0\n")
            .with_context(|| format!("initializing mailbox for {name}"))?;
        Ok(())
    }

    pub(crate) fn rename_session(&self, old: &str, new: &str) -> Result<()> {
        let old_dir = self.mailbox_dir(&Mailbox::Session(old.to_string()));
        let new_mailbox = Mailbox::Session(new.to_string());
        let new_dir = self.mailbox_dir(&new_mailbox);
        remove_dir_if_present(&new_dir)?;
        if old_dir.exists() {
            if let Some(parent) = new_dir.parent() {
                create_private_dir_all(parent)?;
            }
            std::fs::rename(&old_dir, &new_dir).with_context(|| {
                format!(
                    "moving mailbox {} to {}",
                    old_dir.display(),
                    new_dir.display()
                )
            })?;
        } else {
            self.ensure_mailbox(&new_mailbox)?;
        }
        Ok(())
    }

    pub(crate) fn archive_session(
        &self,
        name: &str,
        exited_at: i64,
    ) -> Result<Option<ArchivedMailbox>> {
        let live = self.mailbox_dir(&Mailbox::Session(name.to_string()));
        if !live.exists() {
            return Ok(None);
        }
        create_private_dir_all(&self.archive_root)?;
        let marker = live.join(ARCHIVE_TIMESTAMP);
        write_sync_replace(&marker, format!("{exited_at}\n").as_bytes())?;
        let base = format!(
            "{}.{exited_at}.{}",
            session_component(name),
            std::process::id()
        );
        let mut archived = self.archive_root.join(&base);
        for suffix in 2.. {
            if !archived.exists() {
                break;
            }
            archived = self.archive_root.join(format!("{base}.{suffix}"));
        }
        std::fs::rename(&live, &archived).with_context(|| {
            let _ = std::fs::remove_file(&marker);
            format!(
                "archiving mailbox {} to {}",
                live.display(),
                archived.display()
            )
        })?;
        Ok(Some(ArchivedMailbox { live, archived }))
    }

    pub(crate) fn restore_archived(&self, archived: ArchivedMailbox) -> Result<()> {
        if archived.live.exists() {
            bail!(
                "cannot restore archived mailbox because {} already exists",
                archived.live.display()
            );
        }
        if let Some(parent) = archived.live.parent() {
            create_private_dir_all(parent)?;
        }
        std::fs::rename(&archived.archived, &archived.live).with_context(|| {
            format!(
                "restoring mailbox {} to {}",
                archived.archived.display(),
                archived.live.display()
            )
        })?;
        let _ = std::fs::remove_file(archived.live.join(ARCHIVE_TIMESTAMP));
        Ok(())
    }

    pub(crate) fn prune_archives(&self, hours: u64) -> Result<usize> {
        if hours == 0 {
            return Ok(0);
        }
        let entries = match std::fs::read_dir(&self.archive_root) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("reading {}", self.archive_root.display()));
            }
        };
        let cutoff = session::now_epoch() - (hours as i64) * 3600;
        let mut removed = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let exited_at = std::fs::read_to_string(path.join(ARCHIVE_TIMESTAMP))
                .ok()
                .and_then(|raw| raw.trim().parse::<i64>().ok());
            if exited_at.is_some_and(|value| value < cutoff) {
                std::fs::remove_dir_all(&path)
                    .with_context(|| format!("pruning {}", path.display()))?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub(crate) fn clear_archives(&self) -> Result<()> {
        remove_dir_if_present(&self.archive_root)
    }
}

#[derive(Debug)]
pub(crate) struct ArchivedMailbox {
    live: PathBuf,
    archived: PathBuf,
}

fn atomic_write_new(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .context("mail path is not valid UTF-8")?;
    let base = format!(".{file_name}.{}.tmp", std::process::id());
    let mut temp = parent.join(&base);
    let mut file = None;
    for suffix in 2.. {
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)
        {
            Ok(opened) => {
                file = Some(opened);
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                temp = parent.join(format!("{base}.{suffix}"));
            }
            Err(err) => {
                return Err(err).with_context(|| format!("creating {}", temp.display()));
            }
        }
    }
    let mut file = file.context("could not allocate a temporary mail file")?;
    let publish = (|| -> Result<()> {
        file.write_all(contents)
            .with_context(|| format!("writing {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("syncing {}", temp.display()))?;
        std::fs::rename(&temp, path)
            .with_context(|| format!("publishing mail {} as {}", temp.display(), path.display()))
    })();
    if publish.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    publish
}

fn write_sync_replace(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    file.write_all(contents)
        .with_context(|| format!("writing {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("syncing {}", path.display()))
}

fn remove_dir_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
    }
}

fn local_id(id: &str) -> Option<&str> {
    let id = id.split_once('@').map_or(id, |(local, _)| local);
    let digits = id.strip_prefix('m')?;
    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(id)
}

fn required_inbox(
    store: &MailStore,
    mailbox: &Mailbox,
    id: &str,
) -> Result<(PathBuf, String, Message)> {
    if local_id(id).is_none() {
        die(code::NOT_FOUND, format!("mail not found: {id}"));
    }
    match store.read_inbox(mailbox, id) {
        Ok(message) => Ok(message),
        Err(err)
            if err
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            die(code::NOT_FOUND, format!("mail not found: {id}"));
        }
        Err(err) => Err(err),
    }
}

fn caller_mailbox(ctx: &Ctx) -> Option<Mailbox> {
    std::env::var_os("TMUX")?;
    let pane = std::env::var("TMUX_PANE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let pane = family::canonical_pane(&ctx.tmux, &pane)?;
    if let Some(name) = managed_session_for_pane(ctx, &pane) {
        Some(Mailbox::Session(name))
    } else {
        Some(Mailbox::Pane(pane))
    }
}

fn managed_session_for_pane(ctx: &Ctx, pane: &str) -> Option<String> {
    let name = ctx
        .tmux
        .run(["display-message", "-p", "-t", pane, "#{session_name}"])
        .ok()?
        .trim()
        .to_string();
    session::list(&ctx.tmux)
        .ok()?
        .into_iter()
        .any(|session| session.name == name)
        .then_some(name)
}

fn own_mailbox(ctx: &Ctx) -> Mailbox {
    caller_mailbox(ctx).unwrap_or_else(|| {
        die(
            code::USAGE,
            "no current mailbox; run inside tmux or select a session with -t",
        )
    })
}

fn session_recipient(ctx: &Ctx, raw: &str) -> Recipient {
    let name = session::resolve_existing_name(&ctx.tmux, &ctx.cfg, raw);
    if !session::exists(&ctx.tmux, &name) {
        no_such_session(&name);
    }
    let ping_pane = session::origin_pane(&ctx.tmux, &name).unwrap_or_else(|| name.clone());
    Recipient {
        mailbox: Mailbox::Session(name),
        ping_pane,
    }
}

fn resolve_send_target(ctx: &Ctx, raw: &str) -> Recipient {
    if family::is_parent_target(raw) {
        let pane = family::resolve_parent_pane(ctx);
        if let Some(name) = managed_session_for_pane(ctx, &pane) {
            let ping_pane = session::origin_pane(&ctx.tmux, &name).unwrap_or_else(|| pane.clone());
            return Recipient {
                mailbox: Mailbox::Session(name),
                ping_pane,
            };
        }
        return Recipient {
            mailbox: Mailbox::Pane(pane.clone()),
            ping_pane: pane,
        };
    }
    session_recipient(ctx, raw)
}

fn resolve_reply_target(ctx: &Ctx, from: &str) -> Recipient {
    if let Some(pane) = from.strip_prefix("pane:") {
        let pane = family::canonical_pane(&ctx.tmux, pane)
            .unwrap_or_else(|| die(code::NOT_FOUND, "reply target pane is gone"));
        return Recipient {
            mailbox: Mailbox::Pane(pane.clone()),
            ping_pane: pane,
        };
    }
    if from == "local" {
        die(code::NOT_FOUND, "cannot reply to a local sender");
    }
    session_recipient(ctx, from)
}

fn selected_mailbox(ctx: &Ctx, explicit: Option<&str>) -> Mailbox {
    match explicit {
        Some(target) => session_recipient(ctx, target).mailbox,
        None => own_mailbox(ctx),
    }
}

fn read_body(message: Option<&str>, file: Option<&Path>, stdin: bool) -> Result<String> {
    if let Some(message) = message {
        return Ok(message.to_string());
    }
    if let Some(file) = file {
        return std::fs::read_to_string(file)
            .with_context(|| format!("reading {}", file.display()));
    }
    if stdin {
        let mut body = String::new();
        std::io::stdin()
            .read_to_string(&mut body)
            .context("reading mail from stdin")?;
        return Ok(body);
    }
    Ok(String::new())
}

fn ping_excerpt(message: &Message) -> String {
    let raw = message
        .subject
        .as_deref()
        .unwrap_or_else(|| message.body.lines().next().unwrap_or(""));
    crate::watch::sanitized_line(raw, 80)
}

fn short_sender(sender: &str) -> &str {
    sender.rsplit('/').next().unwrap_or(sender)
}

struct Outgoing {
    body: String,
    subject: Option<String>,
    in_reply_to: Option<String>,
    no_ping: bool,
    quiet: bool,
}

fn send_to(
    ctx: &Ctx,
    sender: Mailbox,
    recipient: Recipient,
    outgoing: Outgoing,
) -> Result<PathBuf> {
    let socket = ctx.tmux.store_socket();
    let store = MailStore::new(&ctx.paths, socket.as_deref());
    store.validate_send_root()?;
    let message = Message {
        from: sender.address(),
        to: recipient.address(),
        date: utc_now(),
        id: String::new(),
        in_reply_to: outgoing.in_reply_to,
        subject: outgoing.subject,
        body: outgoing.body,
    };

    let (inbox_path, inbox_message) = store.write_dual(&sender, &recipient.mailbox, message)?;

    if !outgoing.no_ping {
        let id = local_id(&inbox_message.id).unwrap_or(&inbox_message.id);
        let sender = crate::watch::sanitized_line(short_sender(&inbox_message.from), 80);
        let excerpt = ping_excerpt(&inbox_message);
        let ping = format!(
            "[tpp mail] {id} from {sender}: {excerpt} — read: {}",
            inbox_path.display()
        );
        if let Err(err) = deliver_paste(
            &ctx.tmux,
            &recipient.ping_pane,
            &recipient.address(),
            &ping,
            true,
            ctx.cfg.send.enter_delay_ms,
            false,
        ) {
            eprintln!(
                "tpp: warning: mail written but ping to {} failed: {err}",
                recipient.address()
            );
        }
    }

    if !outgoing.quiet {
        println!("{}", inbox_path.display());
    }
    Ok(inbox_path)
}

fn clap_or_exit<T>(parsed: std::result::Result<T, clap::Error>) -> T {
    parsed.unwrap_or_else(|error| error.exit())
}

pub fn mail(ctx: &Ctx, args: MailArgs) -> Result<()> {
    match args.target_or_verb.as_str() {
        "ls" => {
            let args = clap_or_exit(MailLsArgs::try_parse_from(args.args));
            list(ctx, args)
        }
        "read" => {
            let args = clap_or_exit(MailReadArgs::try_parse_from(args.args));
            read(ctx, args)
        }
        "send" => {
            let args = clap_or_exit(MailSendArgs::try_parse_from(args.args));
            send(ctx, args)
        }
        verb if is_reserved_verb(verb) => unreachable!("reserved mail verb handled above"),
        target => {
            let raw = std::iter::once(target.to_string()).chain(args.args);
            let args = clap_or_exit(MailSendArgs::try_parse_from(raw));
            send(ctx, args)
        }
    }
}

fn send(ctx: &Ctx, args: MailSendArgs) -> Result<()> {
    let body = read_body(args.message.as_deref(), args.file.as_deref(), args.stdin)?;
    let sender = caller_mailbox(ctx).unwrap_or(Mailbox::Local);
    let recipient = resolve_send_target(ctx, &args.target);
    send_to(
        ctx,
        sender,
        recipient,
        Outgoing {
            body,
            subject: args.subject,
            in_reply_to: None,
            no_ping: args.no_ping,
            quiet: ctx.quiet || args.quiet,
        },
    )?;
    Ok(())
}

pub fn reply(ctx: &Ctx, args: ReplyArgs) -> Result<()> {
    let sender = own_mailbox(ctx);
    let socket = ctx.tmux.store_socket();
    let store = MailStore::new(&ctx.paths, socket.as_deref());
    let (_, _, original) = required_inbox(&store, &sender, &args.id)?;
    let recipient = resolve_reply_target(ctx, &original.from);
    let body = read_body(args.message.as_deref(), args.file.as_deref(), args.stdin)?;
    send_to(
        ctx,
        sender,
        recipient,
        Outgoing {
            body,
            subject: None,
            in_reply_to: Some(original.id),
            no_ping: args.no_ping,
            quiet: ctx.quiet,
        },
    )?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct MailListRow {
    id: String,
    folder: String,
    unread: bool,
    from: String,
    to: String,
    date: String,
    subject: Option<String>,
    path: PathBuf,
}

fn list(ctx: &Ctx, args: MailLsArgs) -> Result<()> {
    let mailbox = selected_mailbox(ctx, args.target.as_deref());
    let socket = ctx.tmux.store_socket();
    let store = MailStore::new(&ctx.paths, socket.as_deref());
    let mut rows = Vec::new();
    for folder in [Folder::Inbox, Folder::Sent] {
        let dir = store.mailbox_dir(&mailbox).join(folder.name());
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err).with_context(|| format!("reading {}", dir.display())),
        };
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            let path = entry.path();
            if path.extension() != Some(OsStr::new("md")) {
                continue;
            }
            let Some(id) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            let unread = matches!(folder, Folder::Inbox) && store.is_unread(&mailbox, id);
            if args.unread && !unread {
                continue;
            }
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let message =
                Message::parse(&raw).with_context(|| format!("parsing {}", path.display()))?;
            rows.push(MailListRow {
                id: id.to_string(),
                folder: folder.name().to_string(),
                unread,
                from: message.from,
                to: message.to,
                date: message.date,
                subject: message.subject,
                path,
            });
        }
    }
    rows.sort_by_key(|row| {
        local_id(&row.id)
            .and_then(|id| id[1..].parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });

    if ctx.json || args.json {
        return print_json(&rows);
    }
    if ctx.quiet || args.quiet {
        for row in rows {
            println!("{}", row.id);
        }
        return Ok(());
    }
    for row in rows {
        let state = if row.folder == "sent" {
            "sent"
        } else if row.unread {
            "unread"
        } else {
            "read"
        };
        let subject = row.subject.as_deref().unwrap_or("(no subject)");
        println!(
            "{}  {:<6}  {}  from {}  {}",
            row.id, state, row.date, row.from, subject
        );
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct MailReadOutput {
    path: PathBuf,
    message: Message,
}

fn read(ctx: &Ctx, args: MailReadArgs) -> Result<()> {
    let mailbox = selected_mailbox(ctx, args.target.as_deref());
    let socket = ctx.tmux.store_socket();
    let store = MailStore::new(&ctx.paths, socket.as_deref());
    let (path, raw, message) = required_inbox(&store, &mailbox, &args.id)?;
    store.mark_read(&mailbox, &args.id)?;
    if ctx.json || args.json {
        print_json(&MailReadOutput { path, message })
    } else {
        print!("{raw}");
        if !raw.ends_with('\n') {
            println!();
        }
        Ok(())
    }
}

pub(crate) fn initialize_session(ctx: &Ctx, name: &str) -> Result<()> {
    let socket = ctx.tmux.store_socket();
    MailStore::new(&ctx.paths, socket.as_deref()).reset_session(name)
}

pub(crate) fn rename_session(ctx: &Ctx, old: &str, new: &str) -> Result<()> {
    let socket = ctx.tmux.store_socket();
    MailStore::new(&ctx.paths, socket.as_deref()).rename_session(old, new)
}

pub(crate) fn archive_session(ctx: &Ctx, name: &str) -> Result<Option<ArchivedMailbox>> {
    let socket = ctx.tmux.store_socket();
    MailStore::new(&ctx.paths, socket.as_deref()).archive_session(name, session::now_epoch())
}

pub(crate) fn restore_session(ctx: &Ctx, archived: ArchivedMailbox) -> Result<()> {
    let socket = ctx.tmux.store_socket();
    MailStore::new(&ctx.paths, socket.as_deref()).restore_archived(archived)
}

pub(crate) fn is_reserved_verb(word: &str) -> bool {
    RESERVED_VERBS.contains(&word)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use super::*;

    fn paths(root: &Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
        }
    }

    fn message() -> Message {
        Message {
            from: "tpp/snazzy-otter-0730".to_string(),
            to: "tpp/quirky-gecko-0730".to_string(),
            date: "2026-07-30T14:12:03Z".to_string(),
            id: "m7@tpp/quirky-gecko-0730".to_string(),
            in_reply_to: Some("m3@tpp/snazzy-otter-0730".to_string()),
            subject: Some("Status".to_string()),
            body: "all green\n\nDetails.".to_string(),
        }
    }

    #[test]
    fn header_roundtrip_preserves_thread_and_body() {
        let original = message();
        let parsed = Message::parse(&original.render()).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn ids_are_monotonic_and_safe_under_concurrent_writers() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(MailStore::new(&paths(tmp.path()), None));
        let mailbox = Mailbox::Session("tpp/worker".to_string());
        let mut threads = Vec::new();
        for _ in 0..12 {
            let store = Arc::clone(&store);
            let mailbox = mailbox.clone();
            threads.push(std::thread::spawn(move || {
                let (_, written) = store
                    .write_copy(&mailbox, Folder::Inbox, message())
                    .unwrap();
                written.id
            }));
        }
        let ids: HashSet<String> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(ids.len(), 12);
        assert!(ids.contains("m1@tpp/worker"));
        assert!(ids.contains("m12@tpp/worker"));
    }

    #[test]
    fn read_markers_remove_messages_from_unread_count() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MailStore::new(&paths(tmp.path()), None);
        let mailbox = Mailbox::Session("tpp/worker".to_string());
        let (_, written) = store
            .write_copy(&mailbox, Folder::Inbox, message())
            .unwrap();
        assert_eq!(store.unread_count(&mailbox).unwrap(), 1);
        store.mark_read(&mailbox, &written.id).unwrap();
        assert_eq!(store.unread_count(&mailbox).unwrap(), 0);
    }

    #[test]
    fn reserved_session_components_cannot_overlap_pane_mailboxes() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MailStore::new(&paths(tmp.path()), None);
        let pane = Mailbox::Pane("%7".to_string());
        store.write_copy(&pane, Folder::Inbox, message()).unwrap();

        for name in ["panes", ".", ".."] {
            store.reset_session(name).unwrap();
            assert_eq!(
                store.unread_count(&pane).unwrap(),
                1,
                "resetting {name:?} damaged the pane namespace"
            );
        }
        assert_ne!(
            store.mailbox_dir(&Mailbox::Session("panes".to_string())),
            store.root.join("panes")
        );
    }

    #[test]
    fn recipient_failure_rolls_back_the_sender_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MailStore::new(&paths(tmp.path()), None);
        let sender = Mailbox::Session("tpp/sender".to_string());
        let recipient = Mailbox::Session("tpp/recipient".to_string());
        store.ensure_mailbox(&sender).unwrap();
        std::fs::write(store.mailbox_dir(&recipient), "not a directory").unwrap();

        let error = store
            .write_dual(&sender, &recipient, message())
            .unwrap_err();
        assert!(error.to_string().contains("rolled back"));
        let sent = store.mailbox_dir(&sender).join("sent");
        assert!(std::fs::read_dir(sent)
            .unwrap()
            .all(|entry| entry.unwrap().path().extension() != Some(OsStr::new("md"))));
    }

    #[test]
    fn mail_roots_are_absolute_and_control_characters_are_rejected() {
        let relative = Paths {
            config_dir: PathBuf::new(),
            state_dir: PathBuf::from("relative-mail-state"),
        };
        assert!(MailStore::new(&relative, None).root.is_absolute());

        let invalid = Paths {
            config_dir: PathBuf::new(),
            state_dir: PathBuf::from("mail\nstate"),
        };
        assert!(MailStore::new(&invalid, None).validate_send_root().is_err());
    }

    #[test]
    fn archive_failure_leaves_the_live_mailbox_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MailStore::new(&paths(tmp.path()), None);
        let mailbox = Mailbox::Session("tpp/worker".to_string());
        store.reset_session("tpp/worker").unwrap();
        std::fs::create_dir(store.mailbox_dir(&mailbox).join(ARCHIVE_TIMESTAMP)).unwrap();

        assert!(store.archive_session("tpp/worker", 123).is_err());
        assert!(store.mailbox_dir(&mailbox).exists());
    }

    #[test]
    fn reserved_mail_words_are_not_targets() {
        for verb in ["ls", "read", "send"] {
            assert!(is_reserved_verb(verb));
        }
        assert!(!is_reserved_verb("worker"));
    }
}
