# SysWatch v2.0 — GROUPE

Serveur TCP de surveillance et de contrôle système à distance, écrit en Rust.  
Inspiré du modèle SysWatch v1.0 (ENSPD 2026), enrichi avec de nombreuses fonctionnalités supplémentaires.

---

## Fonctionnalités

### Informations collectées sur la machine hôte

| Commande | Description |
|---|---|
| `host` / `info` | Hostname, OS, version noyau, IP locale, user connecté, architecture CPU, uptime |
| `cpu` | Usage global, nombre de cœurs, modèle, fréquence (MHz) + barre ASCII |
| `mem` / `ram` | RAM totale/utilisée/libre + SWAP + barres ASCII |
| `disk` / `disques` | Toutes les partitions : nom, point de montage, espace total/utilisé/libre, type FS |
| `net` / `reseau` | Toutes les interfaces réseau : octets reçus et envoyés |
| `ps` / `procs` | Top 10 processus par consommation CPU (PID, nom, CPU%, RAM, état, user) |
| `uptime` | Durée depuis le dernier démarrage (jours, heures, minutes) |
| `all` | Vue complète (toutes les catégories ci-dessus) |

### Contrôle de la machine distante

| Commande | Effet |
|---|---|
| `shutdown` | Planifie l'extinction (10 s Windows / 1 min Linux) |
| `reboot` | Planifie le redémarrage |
| `abort` | Annule un shutdown/reboot en cours |
| `lock` | Verrouille la session utilisateur |
| `hibernate` | Met la machine en hibernation |
| `kill <PID>` | Termine un processus par son PID (SIGKILL) |

### Commandes distantes

| Commande | Effet |
|---|---|
| `msg <texte>` | Affiche un message dans le terminal de la machine cible |
| `exec <commande>` | Exécute une commande shell et retourne stdout/stderr |
| `install <paquet>` | Installe un paquet (winget sur Windows, apt-get sur Linux) en arrière-plan |
| `uninstall <paquet>` | Désinstalle un paquet en arrière-plan |
| `wget <url>` | Lance un téléchargement en arrière-plan |

---

## Architecture

```
main.rs
 ├── Types métier        : CpuInfo, MemInfo, DiskInfo, NetworkInfo, HostInfo,
 │                         ProcessInfo, SystemSnapshot
 ├── Trait Display       : affichage humain pour chaque type
 ├── SysWatchError       : enum d'erreurs custom
 ├── collect_snapshot()  : collecte complète via sysinfo + whoami + local-ip-address
 ├── format_response()   : routeur de commandes → réponse textuelle
 ├── handle_client()     : gestion d'un client TCP (auth + boucle commandes)
 ├── snapshot_refresher(): thread de rafraîchissement toutes les 5 s
 └── main()              : démarrage du serveur, Arc<Mutex<SystemSnapshot>>
```

**Concurrence** : chaque client est servi dans son propre thread.  
Le `SystemSnapshot` est partagé via `Arc<Mutex<T>>` entre tous les threads.  
Un thread dédié rafraîchit les métriques toutes les 5 secondes.  
Les commandes d'information (`cpu`, `mem`, etc.) déclenchent en plus un rafraîchissement immédiat.

---

## Prérequis

- Rust ≥ 1.75 ([rustup.rs](https://rustup.rs))
- Accès réseau sur le port **7878**
- `nc` (netcat) ou `telnet` côté client

---

## Compilation et lancement

```bash
# Cloner / décompresser le projet
cd syswatch

# Compiler en mode release
cargo build --release

# Lancer le serveur
cargo run --release
# ou directement :
./target/release/syswatch
```

---

## Connexion et utilisation

```bash
# Linux / macOS / WSL / Git Bash
nc <IP_DU_SERVEUR> 7878

# Windows PowerShell (telnet doit être activé)
telnet <IP_DU_SERVEUR> 7878
```

À la connexion, le serveur demande le **token d'authentification** :

```
TOKEN: ENSPD2026
OK — Authentification réussie.

╔══════════════════════════════════════════════════╗
║         SysWatch v2.0 — GROUPE — ENSPD          ║
║   Tape 'help' pour la liste des commandes.      ║
╚══════════════════════════════════════════════════╝

> cpu
[CPU]
CPU : 12.3%  |  8 cœurs  |  2400 MHz  |  Modèle: Intel Core i7-...
[██████░░░░░░░░░░░░░░░░░░░░░░░░] 12.3%

END
> 
```

---

## Journal d'activité

Toutes les connexions, déconnexions et commandes sont journalisées dans `syswatch.log` :

```
[2026-01-15 14:32:01] [+] Connexion de 192.168.1.42:54321
[2026-01-15 14:32:02] [OK] Authentifié: 192.168.1.42:54321
[2026-01-15 14:32:05] [192.168.1.42:54321] commande: 'cpu'
[2026-01-15 14:32:10] [-] Déconnexion de 192.168.1.42:54321
```

---

## Dépendances (Cargo.toml)

| Crate | Version | Rôle |
|---|---|---|
| `sysinfo` | 0.30 | CPU, RAM, disques, réseau, processus |
| `chrono` | 0.4 | Horodatage |
| `whoami` | 1.5 | Nom de l'utilisateur connecté |
| `local-ip-address` | 0.6 | Adresse IP locale |

---

## Sécurité

- Authentification par token avant toute commande
- Toutes les actions sont journalisées avec horodatage et IP source
- Le token est défini dans la constante `AUTH_TOKEN` dans `main.rs`

> **Note** : Ce projet est à visée pédagogique (ENSPD). En production, utiliser TLS et un mécanisme d'authentification robuste.

---

## Auteur

**GROUPE** — ENSPD 2026
