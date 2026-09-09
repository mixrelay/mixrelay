#!/usr/bin/env bash
set -euo pipefail

# UTM guest-exec may return before a long process exits. The caller must read
# exit-code, never interpret utmctl's immediate exit status as job completion.
job_id=${1:?Expected unique job ID}
shift
[[ "$job_id" =~ ^[a-zA-Z0-9_-]+$ ]] || exit 2
(( $# > 0 )) || exit 2
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
job_root=${TONENARI_LAB_JOB_ROOT:-$repo_root/.lab/jobs}
[[ "$job_root" == /* ]] || exit 2
job_dir="$job_root/tonenari-lab-$job_id"
umask 077
mkdir -p "$job_root"
mkdir "$job_dir" # Exclusive: never overwrite evidence from a previous run.
exec >"$job_dir/output.log" 2>&1
trap 'result=$?; printf "%s\n" "$result" >"$job_dir/exit-code.tmp"; mv "$job_dir/exit-code.tmp" "$job_dir/exit-code"' EXIT
"$@"
