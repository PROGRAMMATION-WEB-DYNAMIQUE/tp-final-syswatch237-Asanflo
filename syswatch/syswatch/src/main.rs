// src/main.rs — SysWatch v2.0 — GROUPE
//
// Serveur TCP multithreadé de surveillance et contrôle système à distance.
// Authentification par token, snapshot partagé rafraîchi toutes les 5 s,
// journalisation horodatée dans syswatch.log.

use chrono::Local;
use local_ip_address::local_ip;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::{DiskExt, NetworkExt, Process, System, SystemExt, CpuExt, ProcessExt, UserExt};

// ─────────────────────────────────────────────
//  CONFIGURATION
// ─────────────────────────────────────────────

const AUTH_TOKEN: &str = "ENSPD2026";
const BIND_ADDR:  &str = "0.0.0.0:7878";
const REFRESH_SECS: u64 = 5;

// ─────────────────────────────────────────────
//  TYPES MÉTIER
// ─────────────────────────────────────────────

#[derive(Debug, Clone)]
struct CpuInfo {
    usage_percent: f32,
    core_count:    usize,
    model:         String,
    frequency_mhz: u64,
}

#[derive(Debug, Clone)]
struct MemInfo {
    total_mb: u64,
    used_mb:  u64,
    free_mb:  u64,
    swap_total_mb: u64,
    swap_used_mb:  u64,
}

#[derive(Debug, Clone)]
struct DiskInfo {
    name:       String,
    mount:      String,
    total_gb:   f64,
    used_gb:    f64,
    free_gb:    f64,
    usage_pct:  f64,
    fs_type:    String,
}

#[derive(Debug, Clone)]
struct NetworkInfo {
    iface:        String,
    received_mb:  f64,
    sent_mb:      f64,
}

#[derive(Debug, Clone)]
struct HostInfo {
    hostname:   String,
    os_name:    String,
    os_version: String,
    kernel:     String,
    uptime_sec: u64,
    local_ip:   String,
    username:   String,
    cpu_arch:   String,
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    pid:       u32,
    name:      String,
    cpu_usage: f32,
    memory_mb: u64,
    status:    String,
    user:      String,
}

#[derive(Debug, Clone)]
struct SystemSnapshot {
    timestamp: String,
    host:      HostInfo,
    cpu:       CpuInfo,
    memory:    MemInfo,
    disks:     Vec<DiskInfo>,
    networks:  Vec<NetworkInfo>,
    top_processes: Vec<ProcessInfo>,
}

// ─────────────────────────────────────────────
//  AFFICHAGE (Trait Display)
// ─────────────────────────────────────────────

impl fmt::Display for CpuInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CPU : {:.1}%  |  {} cœurs  |  {} MHz  |  Modèle: {}",
            self.usage_percent, self.core_count, self.frequency_mhz, self.model
        )
    }
}

impl fmt::Display for MemInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RAM : {}MB / {}MB utilisés ({} MB libres) | SWAP: {}MB / {}MB",
            self.used_mb, self.total_mb, self.free_mb,
            self.swap_used_mb, self.swap_total_mb
        )
    }
}

impl fmt::Display for DiskInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  {:>10}  {}  {:.1}GB / {:.1}GB  ({:.1}%)  [{}]",
            self.name, self.mount,
            self.used_gb, self.total_gb,
            self.usage_pct, self.fs_type
        )
    }
}

impl fmt::Display for NetworkInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  {:>12}  ↓ {:.2} MB reçus  ↑ {:.2} MB envoyés",
            self.iface, self.received_mb, self.sent_mb
        )
    }
}

impl fmt::Display for HostInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Hôte: {}  |  OS: {} {}  |  Kernel: {}\nIP: {}  |  User: {}  |  Arch: {}  |  Uptime: {}",
            self.hostname, self.os_name, self.os_version, self.kernel,
            self.local_ip, self.username, self.cpu_arch,
            format_uptime(self.uptime_sec)
        )
    }
}

impl fmt::Display for ProcessInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  [{:>6}] {:<25} CPU:{:>5.1}%  MEM:{:>5}MB  État:{:<8}  User:{}",
            self.pid, self.name, self.cpu_usage, self.memory_mb, self.status, self.user
        )
    }
}

impl fmt::Display for SystemSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "╔══════════════════════════════════════════════╗")?;
        writeln!(f, "║         SysWatch v2.0 — Vue complète         ║")?;
        writeln!(f, "║  {}  ║", self.timestamp)?;
        writeln!(f, "╚══════════════════════════════════════════════╝")?;
        writeln!(f, "\n[HÔTE]\n{}", self.host)?;
        writeln!(f, "\n[CPU]\n{}", self.cpu)?;
        writeln!(f, "\n[MÉMOIRE]\n{}", self.memory)?;
        writeln!(f, "\n[DISQUES]")?;
        for d in &self.disks { writeln!(f, "{}", d)?; }
        writeln!(f, "\n[RÉSEAU]")?;
        for n in &self.networks { writeln!(f, "{}", n)?; }
        writeln!(f, "\n[TOP PROCESSUS]")?;
        for p in &self.top_processes { writeln!(f, "{}", p)?; }
        write!(f, "══════════════════════════════════════════════════")
    }
}

// ─────────────────────────────────────────────
//  ERREURS CUSTOM
// ─────────────────────────────────────────────

#[derive(Debug)]
enum SysWatchError {
    CollectionFailed(String),
}

impl fmt::Display for SysWatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SysWatchError::CollectionFailed(msg) => write!(f, "Erreur collecte: {}", msg),
        }
    }
}

impl std::error::Error for SysWatchError {}

// ─────────────────────────────────────────────
//  UTILITAIRES
// ─────────────────────────────────────────────

fn format_uptime(secs: u64) -> String {
    let days  = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins  = (secs % 3600) / 60;
    format!("{}j {}h {}m", days, hours, mins)
}

fn ascii_bar(pct: f64, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f64) as usize;
    let empty  = width.saturating_sub(filled);
    format!("[{}{}] {:.1}%", "█".repeat(filled), "░".repeat(empty), pct)
}

fn log_event(message: &str) {
    let ts   = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("[{}] {}\n", ts, message);
    print!("{}", line);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("syswatch.log") {
        let _ = f.write_all(line.as_bytes());
    }
}

// ─────────────────────────────────────────────
//  COLLECTE SYSTÈME
// ─────────────────────────────────────────────

fn collect_snapshot() -> Result<SystemSnapshot, SysWatchError> {
    let mut sys = System::new_all();
    sys.refresh_all();
    thread::sleep(Duration::from_millis(600));
    sys.refresh_all();

    // ── CPU ──────────────────────────────────
    let core_count = sys.cpus().len();
    if core_count == 0 {
        return Err(SysWatchError::CollectionFailed("Aucun CPU détecté".into()));
    }
    let cpu_usage   = sys.global_cpu_info().cpu_usage();
    let cpu_model   = sys.global_cpu_info().brand().trim().to_string();
    let cpu_freq    = sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);

    // ── Mémoire ──────────────────────────────
    let total_mb      = sys.total_memory()  / 1024 / 1024;
    let used_mb       = sys.used_memory()   / 1024 / 1024;
    let free_mb       = sys.free_memory()   / 1024 / 1024;
    let swap_total_mb = sys.total_swap()    / 1024 / 1024;
    let swap_used_mb  = sys.used_swap()     / 1024 / 1024;

    // ── Disques ──────────────────────────────
    let disks: Vec<DiskInfo> = sys.disks().iter().map(|d| {
        let total  = d.total_space()     as f64 / 1e9;
        let free   = d.available_space() as f64 / 1e9;
        let used   = total - free;
        let pct    = if total > 0.0 { used / total * 100.0 } else { 0.0 };
        DiskInfo {
            name:      d.name().to_string_lossy().to_string(),
            mount:     d.mount_point().to_string_lossy().to_string(),
            total_gb:  total,
            used_gb:   used,
            free_gb:   free,
            usage_pct: pct,
            fs_type:   d.file_system().iter().map(|b| *b as char).collect(),
        }
    }).collect();

    // ── Réseau ───────────────────────────────
    let networks: Vec<NetworkInfo> = sys.networks().iter()
        .filter(|(name, _)| !name.starts_with("lo"))
        .map(|(name, data)| NetworkInfo {
            iface:       name.clone(),
            received_mb: data.total_received()    as f64 / 1024.0 / 1024.0,
            sent_mb:     data.total_transmitted() as f64 / 1024.0 / 1024.0,
        })
        .collect();

    // ── Hôte ─────────────────────────────────
    let ip_str = local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "N/A".into());

    let username = whoami::username();
    let cpu_arch = std::env::consts::ARCH.to_string();

    let host = HostInfo {
        hostname:   System::host_name().unwrap_or_else(|| "N/A".into()),
        os_name:    System::name().unwrap_or_else(|| "N/A".into()),
        os_version: System::os_version().unwrap_or_else(|| "N/A".into()),
        kernel:     System::kernel_version().unwrap_or_else(|| "N/A".into()),
        uptime_sec: System::uptime(),
        local_ip:   ip_str,
        username,
        cpu_arch,
    };

    // ── Processus (top 10 CPU) ────────────────
    let mut processes: Vec<ProcessInfo> = sys.processes().values()
        .map(|p: &Process| {
            let status = format!("{:?}", p.status());
            let user   = p.user_id()
                .and_then(|uid| sys.get_user_by_id(uid))
                .map(|u| u.name().to_string())
                .unwrap_or_else(|| "-".into());
            ProcessInfo {
                pid:       p.pid().as_u32(),
                name:      p.name().to_string(),
                cpu_usage: p.cpu_usage(),
                memory_mb: p.memory() / 1024 / 1024,
                status,
                user,
            }
        })
        .collect();

    processes.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap());
    processes.truncate(10);

    Ok(SystemSnapshot {
        timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        host,
        cpu: CpuInfo {
            usage_percent: cpu_usage,
            core_count,
            model: cpu_model,
            frequency_mhz: cpu_freq,
        },
        memory: MemInfo { total_mb, used_mb, free_mb, swap_total_mb, swap_used_mb },
        disks,
        networks,
        top_processes: processes,
    })
}

// ─────────────────────────────────────────────
//  FORMATAGE DES RÉPONSES
// ─────────────────────────────────────────────

fn format_response(snapshot: &SystemSnapshot, command: &str) -> String {
    let cmd = command.trim().to_lowercase();

    match cmd.as_str() {

        // ── Informations système ──────────────
        "host" | "info" => {
            format!("[HÔTE]\n{}\n", snapshot.host)
        }

        "cpu" => {
            let pct = snapshot.cpu.usage_percent as f64;
            format!(
                "[CPU]\n{}\n{}\n",
                snapshot.cpu,
                ascii_bar(pct, 30)
            )
        }

        "mem" | "ram" => {
            let pct = snapshot.memory.used_mb as f64 / snapshot.memory.total_mb as f64 * 100.0;
            let swap_pct = if snapshot.memory.swap_total_mb > 0 {
                snapshot.memory.swap_used_mb as f64 / snapshot.memory.swap_total_mb as f64 * 100.0
            } else { 0.0 };
            format!(
                "[MÉMOIRE]\n{}\nRAM  {}\nSWAP {}\n",
                snapshot.memory,
                ascii_bar(pct, 30),
                ascii_bar(swap_pct, 30)
            )
        }

        "disk" | "disques" => {
            let header = format!("[DISQUES — {} partition(s)]\n", snapshot.disks.len());
            let body: String = snapshot.disks.iter().map(|d| {
                format!("{}\n  {}\n", d, ascii_bar(d.usage_pct, 20))
            }).collect();
            format!("{}{}", header, body)
        }

        "net" | "reseau" => {
            let header = "[RÉSEAU]\n".to_string();
            let body: String = snapshot.networks.iter()
                .map(|n| format!("{}\n", n))
                .collect();
            format!("{}{}", header, if body.is_empty() { "  Aucune interface réseau.\n".into() } else { body })
        }

        "ps" | "procs" => {
            let lines: String = snapshot.top_processes.iter().enumerate()
                .map(|(i, p)| format!("{}. {}\n", i + 1, p))
                .collect();
            format!("[PROCESSUS — Top {}]\n{}", snapshot.top_processes.len(), lines)
        }

        "uptime" => {
            format!("[UPTIME]\n  {}\n", format_uptime(snapshot.host.uptime_sec))
        }

        "all" | "" => format!("{}\n", snapshot),

        // ── Contrôle système ──────────────────

        "shutdown" => {
            log_event("[!] SHUTDOWN demandé par client");
            if cfg!(target_os = "windows") {
                std::process::Command::new("shutdown").args(["/s", "/t", "10"]).spawn().ok();
            } else {
                std::process::Command::new("shutdown").args(["-h", "+1"]).spawn().ok();
            }
            "SHUTDOWN programmé dans 10 secondes (Windows) / 1 minute (Linux).\n".to_string()
        }

        "reboot" => {
            log_event("[!] REBOOT demandé par client");
            if cfg!(target_os = "windows") {
                std::process::Command::new("shutdown").args(["/r", "/t", "10"]).spawn().ok();
            } else {
                std::process::Command::new("shutdown").args(["-r", "+1"]).spawn().ok();
            }
            "REBOOT programmé dans 10 secondes (Windows) / 1 minute (Linux).\n".to_string()
        }

        "abort" => {
            log_event("[!] ABORT demandé par client");
            if cfg!(target_os = "windows") {
                std::process::Command::new("shutdown").args(["/a"]).spawn().ok();
            } else {
                std::process::Command::new("shutdown").args(["-c"]).spawn().ok();
            }
            "Extinction annulée.\n".to_string()
        }

        "lock" => {
            log_event("[!] LOCK écran demandé par client");
            if cfg!(target_os = "windows") {
                std::process::Command::new("rundll32")
                    .args(["user32.dll,LockWorkStation"])
                    .spawn().ok();
            } else {
                std::process::Command::new("loginctl").args(["lock-session"]).spawn().ok();
            }
            "Session verrouillée.\n".to_string()
        }

        "hibernate" => {
            log_event("[!] HIBERNATE demandé par client");
            if cfg!(target_os = "windows") {
                std::process::Command::new("shutdown").args(["/h"]).spawn().ok();
            } else {
                std::process::Command::new("systemctl").args(["hibernate"]).spawn().ok();
            }
            "Hibernation en cours...\n".to_string()
        }

        // ── kill <PID> ────────────────────────
        _ if cmd.starts_with("kill ") => {
            let pid_str = cmd[5..].trim();
            match pid_str.parse::<u32>() {
                Ok(pid) => {
                    log_event(&format!("[!] KILL PID={} demandé", pid));
                    if cfg!(target_os = "windows") {
                        std::process::Command::new("taskkill")
                            .args(["/PID", &pid.to_string(), "/F"])
                            .spawn().ok();
                    } else {
                        std::process::Command::new("kill")
                            .args(["-9", &pid.to_string()])
                            .spawn().ok();
                    }
                    format!("Signal KILL envoyé au processus PID {}.\n", pid)
                }
                Err(_) => format!("Usage: kill <PID>  (ex: kill 1234)\n"),
            }
        }

        // ── msg <texte> ────────────────────────
        _ if cmd.starts_with("msg ") => {
            let text = &command.trim()[4..];
            println!("\n╔══════════════════════════════════════╗");
            println!("║  MESSAGE DISTANT                     ║");
            println!("║  {}{}║", text, " ".repeat(38usize.saturating_sub(text.len())));
            println!("╚══════════════════════════════════════╝\n");
            log_event(&format!("[MSG] {}", text));
            "Message affiché sur la machine cible.\n".to_string()
        }

        // ── exec <commande> ────────────────────
        _ if cmd.starts_with("exec ") => {
            let raw_cmd = &command.trim()[5..];
            log_event(&format!("[EXEC] {}", raw_cmd));
            let output = if cfg!(target_os = "windows") {
                std::process::Command::new("cmd")
                    .args(["/C", raw_cmd])
                    .output()
            } else {
                std::process::Command::new("sh")
                    .args(["-c", raw_cmd])
                    .output()
            };
            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let mut res = format!("[EXEC] '{}'\n", raw_cmd);
                    if !stdout.is_empty() { res.push_str(&format!("[stdout]\n{}\n", stdout)); }
                    if !stderr.is_empty() { res.push_str(&format!("[stderr]\n{}\n", stderr)); }
                    res
                }
                Err(e) => format!("Erreur exec: {}\n", e),
            }
        }

        // ── install <package> ─────────────────
        _ if cmd.starts_with("install ") => {
            let package = cmd[8..].trim().to_string();
            log_event(&format!("[INSTALL] {}", package));
            std::thread::spawn(move || {
                if cfg!(target_os = "windows") {
                    std::process::Command::new("winget")
                        .args(["install", "--silent", &package])
                        .status().ok();
                } else {
                    std::process::Command::new("apt-get")
                        .args(["-y", "install", &package])
                        .status().ok();
                }
            });
            format!("Installation de '{}' lancée en arrière-plan.\n", &cmd[8..])
        }

        // ── uninstall <package> ───────────────
        _ if cmd.starts_with("uninstall ") => {
            let package = cmd[10..].trim().to_string();
            log_event(&format!("[UNINSTALL] {}", package));
            std::thread::spawn(move || {
                if cfg!(target_os = "windows") {
                    std::process::Command::new("winget")
                        .args(["uninstall", "--silent", &package])
                        .status().ok();
                } else {
                    std::process::Command::new("apt-get")
                        .args(["-y", "remove", &package])
                        .status().ok();
                }
            });
            format!("Désinstallation de '{}' lancée en arrière-plan.\n", &cmd[10..])
        }

        // ── wget <url> ────────────────────────
        _ if cmd.starts_with("wget ") => {
            let url = cmd[5..].trim().to_string();
            log_event(&format!("[WGET] {}", url));
            std::thread::spawn(move || {
                std::process::Command::new("wget")
                    .args(["-q", &url])
                    .status().ok();
            });
            format!("Téléchargement de '{}' lancé en arrière-plan.\n", &cmd[5..])
        }

        // ── Aide ──────────────────────────────
        "help" => concat!(
            "╔══════════════════════════════════════════════════╗\n",
            "║          SysWatch v2.0 — Aide complète          ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║  INFORMATIONS SYSTÈME                           ║\n",
            "║  host / info  — Infos hôte (OS, IP, user…)     ║\n",
            "║  cpu          — Usage CPU + modèle + fréquence  ║\n",
            "║  mem / ram    — RAM + SWAP                      ║\n",
            "║  disk         — Partitions disque               ║\n",
            "║  net / reseau — Interfaces réseau (trafic)      ║\n",
            "║  ps / procs   — Top 10 processus par CPU        ║\n",
            "║  uptime       — Durée depuis démarrage          ║\n",
            "║  all          — Vue complète                    ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║  CONTRÔLE SYSTÈME                               ║\n",
            "║  shutdown     — Éteindre la machine             ║\n",
            "║  reboot       — Redémarrer la machine           ║\n",
            "║  abort        — Annuler shutdown/reboot         ║\n",
            "║  lock         — Verrouiller la session          ║\n",
            "║  hibernate    — Mettre en hibernation           ║\n",
            "║  kill <PID>   — Tuer un processus               ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║  COMMANDES DISTANTES                            ║\n",
            "║  msg  <texte>     — Afficher un message         ║\n",
            "║  exec <commande>  — Exécuter une commande shell  ║\n",
            "║  install   <pkg>  — Installer un paquet         ║\n",
            "║  uninstall <pkg>  — Désinstaller un paquet      ║\n",
            "║  wget <url>       — Télécharger un fichier      ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║  quit / exit  — Fermer la connexion             ║\n",
            "╚══════════════════════════════════════════════════╝\n",
        ).to_string(),

        "quit" | "exit" => "BYE\n".to_string(),

        _ => format!("Commande inconnue: '{}'. Tape 'help'.\n", command.trim()),
    }
}

// ─────────────────────────────────────────────
//  GESTION CLIENT TCP
// ─────────────────────────────────────────────

fn handle_client(mut stream: TcpStream, snapshot: Arc<Mutex<SystemSnapshot>>) {
    let peer = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "inconnu".into());
    log_event(&format!("[+] Connexion de {}", peer));

    // ── Authentification ─────────────────────
    let _ = stream.write_all(b"TOKEN: ");
    let mut reader = BufReader::new(stream.try_clone().expect("Clone stream échoué"));
    let mut token_line = String::new();

    if reader.read_line(&mut token_line).is_err() || token_line.trim() != AUTH_TOKEN {
        let _ = stream.write_all(b"UNAUTHORIZED — Token invalide.\n");
        log_event(&format!("[!] Accès refusé depuis {}", peer));
        return;
    }

    let _ = stream.write_all(b"OK — Authentification réussie.\n");
    log_event(&format!("[OK] Authentifié: {}", peer));

    // ── Message de bienvenue ─────────────────
    let welcome = concat!(
        "\n╔══════════════════════════════════════════════════╗\n",
        "║         SysWatch v2.0 — GROUPE — ENSPD          ║\n",
        "║   Tape 'help' pour la liste des commandes.      ║\n",
        "╚══════════════════════════════════════════════════╝\n\n",
        "> "
    );
    let _ = stream.write_all(welcome.as_bytes());

    // ── Boucle de commandes ───────────────────
    for line in reader.lines() {
        match line {
            Ok(cmd) => {
                let cmd = cmd.trim().to_string();
                log_event(&format!("[{}] commande: '{}'", peer, cmd));

                if cmd.eq_ignore_ascii_case("quit") || cmd.eq_ignore_ascii_case("exit") {
                    let _ = stream.write_all(b"Au revoir!\nBYE\n");
                    break;
                }

                // Pour cpu / mem / disques / all : rafraîchir le snapshot d'abord
                let needs_refresh = matches!(
                    cmd.to_lowercase().as_str(),
                    "cpu" | "mem" | "ram" | "disk" | "disques" | "net" | "reseau" | "ps" | "procs" | "all" | "" | "host" | "info" | "uptime"
                );

                let response = if needs_refresh {
                    match collect_snapshot() {
                        Ok(fresh) => {
                            let mut snap = snapshot.lock().unwrap();
                            *snap = fresh;
                            format_response(&snap, &cmd)
                        }
                        Err(_) => {
                            let snap = snapshot.lock().unwrap();
                            format_response(&snap, &cmd)
                        }
                    }
                } else {
                    let snap = snapshot.lock().unwrap();
                    format_response(&snap, &cmd)
                };

                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(b"\nEND\n> ");
            }
            Err(_) => break,
        }
    }

    log_event(&format!("[-] Déconnexion de {}", peer));
}

// ─────────────────────────────────────────────
//  RAFRAÎCHISSEMENT EN ARRIÈRE-PLAN
// ─────────────────────────────────────────────

fn snapshot_refresher(snapshot: Arc<Mutex<SystemSnapshot>>) {
    loop {
        thread::sleep(Duration::from_secs(REFRESH_SECS));
        match collect_snapshot() {
            Ok(new_snap) => {
                let mut snap = snapshot.lock().unwrap();
                *snap = new_snap;
                log_event("[refresh] Métriques mises à jour");
            }
            Err(e) => eprintln!("[refresh] Erreur: {}", e),
        }
    }
}

// ─────────────────────────────────────────────
//  MAIN
// ─────────────────────────────────────────────

fn main() {
    println!("╔══════════════════════════════════════════════════╗");
    println!("║         SysWatch v2.0 — GROUPE — ENSPD          ║");
    println!("║         Démarrage du serveur TCP...              ║");
    println!("╚══════════════════════════════════════════════════╝\n");

    log_event("=== SysWatch v2.0 démarrage ===");

    // Collecte initiale
    let initial = collect_snapshot().expect("Impossible de collecter les métriques initiales");
    println!("Métriques initiales collectées:\n{}", initial);

    let shared_snapshot = Arc::new(Mutex::new(initial));

    // Thread de rafraîchissement automatique
    {
        let snap_clone = Arc::clone(&shared_snapshot);
        thread::spawn(move || snapshot_refresher(snap_clone));
    }

    // Démarrage du serveur TCP
    let listener = TcpListener::bind(BIND_ADDR)
        .unwrap_or_else(|_| panic!("Impossible de bind sur {}", BIND_ADDR));

    println!("\n Serveur en écoute sur {}", BIND_ADDR);
    println!(" Token d'accès : {}", AUTH_TOKEN);
    println!(" Connexion : nc <IP> 7878  |  telnet <IP> 7878");
    println!(" Log        : syswatch.log");
    println!(" Ctrl+C pour arrêter.\n");
    log_event(&format!("Serveur démarré sur {}", BIND_ADDR));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let snap_clone = Arc::clone(&shared_snapshot);
                thread::spawn(move || handle_client(stream, snap_clone));
            }
            Err(e) => eprintln!("Erreur connexion entrante: {}", e),
        }
    }
}
