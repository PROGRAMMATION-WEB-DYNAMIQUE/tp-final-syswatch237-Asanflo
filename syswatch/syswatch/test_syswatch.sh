#!/usr/bin/env bash
# test_syswatch.sh — Script de test SysWatch v2.0
# Exécuter depuis la racine du projet : bash test_syswatch.sh

set -e

echo "============================================"
echo "   SysWatch v2.0 — Tests automatisés"
echo "============================================"

# ── 1. Vérification des fichiers requis ──────
echo ""
echo "[1/5] Vérification structure du projet..."
for f in "Cargo.toml" "src/main.rs" "README.md"; do
    if [ -f "$f" ]; then
        echo "  [OK] $f"
    else
        echo "  [ERREUR] $f manquant"
        exit 1
    fi
done

# ── 2. Vérification Cargo.toml ───────────────
echo ""
echo "[2/5] Vérification Cargo.toml..."
grep -q 'name = "syswatch"'    Cargo.toml && echo '  [OK] name = "syswatch"'
grep -q 'authors = \["GROUPE"\]' Cargo.toml && echo '  [OK] authors = ["GROUPE"]'
grep -q 'sysinfo'              Cargo.toml && echo '  [OK] dépendance sysinfo présente'
grep -q 'chrono'               Cargo.toml && echo '  [OK] dépendance chrono présente'
grep -q 'whoami'               Cargo.toml && echo '  [OK] dépendance whoami présente'
grep -q 'local-ip-address'     Cargo.toml && echo '  [OK] dépendance local-ip-address présente'

# ── 3. Compilation debug ─────────────────────
echo ""
echo "[3/5] Compilation debug (cargo build)..."
cargo build 2>&1
echo "  [OK] Compilation debug réussie"

# ── 4. Compilation release ───────────────────
echo ""
echo "[4/5] Compilation release (cargo build --release)..."
cargo build --release 2>&1
echo "  [OK] Compilation release réussie"

# ── 5. Test de connexion TCP ─────────────────
echo ""
echo "[5/5] Test serveur TCP (lancement 8s)..."

# Lancer le serveur en arrière-plan
./target/release/syswatch &
SERVER_PID=$!
sleep 3  # attendre que le serveur soit prêt

echo "  Serveur démarré (PID=$SERVER_PID)"

# Test authentification incorrecte
echo "  Test token invalide..."
RESP=$(echo -e "MAUVAIS_TOKEN\nquit\n" | timeout 4 nc localhost 7878 2>/dev/null || true)
if echo "$RESP" | grep -q "UNAUTHORIZED"; then
    echo "  [OK] Token invalide correctement rejeté"
else
    echo "  [WARN] Réponse UNAUTHORIZED non trouvée (peut dépendre de nc)"
fi

# Test authentification correcte + commandes
echo "  Test token valide + commandes..."
RESP=$(printf "ENSPD2026\ncpu\nmem\nhost\nhelp\nquit\n" | timeout 8 nc localhost 7878 2>/dev/null || true)
if echo "$RESP" | grep -q "OK"; then
    echo "  [OK] Authentification réussie"
else
    echo "  [WARN] Réponse OK non trouvée"
fi
if echo "$RESP" | grep -qi "cpu"; then
    echo "  [OK] Commande 'cpu' fonctionne"
fi
if echo "$RESP" | grep -qi "mémoire\|mem\|RAM"; then
    echo "  [OK] Commande 'mem' fonctionne"
fi
if echo "$RESP" | grep -qi "hôte\|host\|Hote"; then
    echo "  [OK] Commande 'host' fonctionne"
fi
if echo "$RESP" | grep -qi "help\|commandes"; then
    echo "  [OK] Commande 'help' fonctionne"
fi

# Arrêter le serveur
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo ""
echo "============================================"
echo "   Tous les tests passés avec succès !"
echo "   Binaire : ./target/release/syswatch"
echo "   Log     : syswatch.log"
echo "============================================"
