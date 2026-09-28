#!/usr/bin/env bash
set -euo pipefail
umask 077
reservation=__CASPER_RESERVATION__
jit=__CASPER_JIT__
[[ $EUID == 0 && "$reservation" =~ ^[a-f0-9]{64}$ ]]
for executable in jq curl iptables newuidmap newgidmap dockerd-rootless.sh loginctl; do
  command -v "$executable" >/dev/null
done
[[ "$(stat -fc %T /sys/fs/cgroup)" == cgroup2fs ]]
[[ -x /opt/actions-runner/run.sh && ! -e /opt/casper-runner ]]
useradd --create-home --home-dir /opt/casper-runner --shell /bin/bash casper
user_id="$(id -u casper)"
[[ "$(id -Gn casper)" == casper ]]
for file in bin externals run.sh run-helper.sh.template env.sh; do
  cp -a -- "/opt/actions-runner/$file" "/opt/casper-runner/$file"
done
chown -R casper:casper /opt/casper-runner
sudo -n -l -U casper > /run/casper-sudo-check 2>&1 || true
if rg -q '\(ALL|NOPASSWD' /run/casper-sudo-check; then exit 2; fi
instance="$(curl --connect-timeout 5 --max-time 10 -fsS -H 'Authorization: Bearer Oracle' http://169.254.169.254/opc/v2/instance/)"
instance_id="$(jq -er .id <<< "$instance")"
[[ "$instance_id" =~ ^ocid1.instance.[a-zA-Z0-9.]+$ ]]
iptables -I OUTPUT 1 -d 169.254.169.254/32 -m owner --uid-owner "$user_id" -j REJECT
iptables -C OUTPUT -d 169.254.169.254/32 -m owner --uid-owner "$user_id" -j REJECT
loginctl enable-linger casper
systemctl start "user@$user_id.service"
runtime="/run/user/$user_id"
export XDG_RUNTIME_DIR="$runtime"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime/bus"
userctl() { runuser -u casper -- env XDG_RUNTIME_DIR="$runtime" DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" systemctl --user "$@"; }
install -d -m 700 -o casper -g casper "/opt/casper-runner/.config/systemd/user" "$runtime/casper-$reservation"
install -d -m 700 -o casper -g casper "/var/lib/casper/$reservation"
install -d -m 755 "/run/casper/$reservation"
printf '%s' '__CASPER_GUARD_BASE64__' | base64 -d > "/run/casper/$reservation/guard.sh"
chmod 755 "/run/casper/$reservation/guard.sh"
units=/opt/casper-runner/.config/systemd/user
cat > "$units/casper-$reservation.slice" <<EOF
[Unit]
Description=Casper workload memory limit
[Slice]
MemoryMax=45056M
MemorySwapMax=0
EOF
cat > "$units/casper-$reservation-docker.service" <<EOF
[Unit]
Description=Casper private rootless Docker
[Service]
Environment=HOME=/opt/casper-runner
Environment=XDG_RUNTIME_DIR=$runtime
Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_STATE_DIR=$runtime/casper-$reservation/rootlesskit
ExecStart=$(command -v dockerd-rootless.sh) --host=unix://$runtime/casper-$reservation/docker.sock --data-root=/var/lib/casper/$reservation/docker --exec-root=$runtime/casper-$reservation/exec
Slice=casper-$reservation.slice
Delegate=yes
KillMode=mixed
TimeoutStopSec=30
Restart=no
EOF
cat > "$units/casper-$reservation-guardian.service" <<EOF
[Unit]
Description=Casper host guardian
[Service]
ExecStart=/bin/bash /run/casper/$reservation/guard.sh $reservation /var/lib/casper/$reservation /var/lib/casper/$reservation
Restart=no
OOMScoreAdjust=0
EOF
chown casper:casper "$units/"*
userctl daemon-reload
userctl start "casper-$reservation.slice" "casper-$reservation-docker.service"
for attempt in {1..60}; do
  [[ -S "$runtime/casper-$reservation/docker.sock" ]] && break
  sleep 1
done
[[ -S "$runtime/casper-$reservation/docker.sock" ]]
userctl start "casper-$reservation-guardian.service"
userctl is-active --quiet "casper-$reservation-guardian.service"
jq -n --arg reservation "$reservation" --arg instance "$instance_id" \
  '{schema_version:1,reservation_id:$reservation,instance_id:$instance,metadata_access_blocked:true,runner_exclusive:true}' \
  > "/run/casper/$reservation/host.json"
chmod 444 "/run/casper/$reservation/host.json"
printf '%s' "$jit" > "/opt/casper-runner/.casper-jit"
chmod 400 /opt/casper-runner/.casper-jit
chown casper:casper /opt/casper-runner/.casper-jit
unset jit instance
cat > "/run/casper/$reservation/runner.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
cd /opt/casper-runner
exec ./run.sh --jitconfig "\$(cat /opt/casper-runner/.casper-jit)"
EOF
chmod 755 "/run/casper/$reservation/runner.sh"
systemd-run --unit "casper-$reservation-runner" --property User=casper \
  --property WorkingDirectory=/opt/casper-runner --property NoNewPrivileges=yes \
  --property OOMScoreAdjust=-900 --property MemoryMax=4G --property MemorySwapMax=0 \
  --setenv HOME=/opt/casper-runner --setenv XDG_RUNTIME_DIR="$runtime" \
  --setenv DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" \
  --setenv DOCKER_HOST="unix://$runtime/casper-$reservation/docker.sock" \
  "/run/casper/$reservation/runner.sh"
