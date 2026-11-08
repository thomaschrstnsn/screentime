# Shared helpers for the test VM scripts. Sourced, not executed.
set -euo pipefail

VM_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_DIR=$(dirname "$VM_DIR")
STATE_DIR=${TEST_VM_STATE:-$VM_DIR/.state}

SSH_PORT=12222
NATS_PORT=14222
VM_USER=${TEST_VM_USER:-junior}

DISK=$STATE_DIR/disk.qcow2
KEY=$STATE_DIR/id_ed25519
PID_FILE=$STATE_DIR/qemu.pid
CONSOLE_LOG=$STATE_DIR/console.log

die() { echo "error: $*" >&2; exit 1; }
info() { echo "==> $*" >&2; }

vm_pid() {
  [[ -f $PID_FILE ]] || return 1
  local pid
  pid=$(<"$PID_FILE")
  kill -0 "$pid" 2>/dev/null || return 1
  echo "$pid"
}

is_running() { vm_pid >/dev/null; }

require_running() { is_running || die "the VM is not running (start it with test-vm/start)"; }

vm_ssh() {
  ssh -q \
    -o BatchMode=yes \
    -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR \
    -o ConnectTimeout=3 \
    -i "$KEY" -p "$SSH_PORT" root@127.0.0.1 "$@"
}

# Parses a duration into seconds: 90, 90s, 10m, 2h, 1h30m.
parse_duration() {
  local input=$1 rest=$1 total=0
  [[ -n $rest ]] || die "empty duration"
  while [[ -n $rest ]]; do
    if [[ $rest =~ ^([0-9]+)([hms]?)(.*)$ ]]; then
      local n=${BASH_REMATCH[1]} unit=${BASH_REMATCH[2]}
      rest=${BASH_REMATCH[3]}
      case $unit in
        h) total=$((total + n * 3600)) ;;
        m) total=$((total + n * 60)) ;;
        s | "") total=$((total + n)) ;;
      esac
    else
      die "not a duration: '$input' (use e.g. 600, 10m, 1h30m)"
    fi
  done
  echo "$total"
}
