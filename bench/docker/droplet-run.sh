#!/bin/bash
# Runs on the droplet (see droplet.ps1): installs Docker and PowerShell, builds the images,
# runs the Docker server bench against Remix, the rmx-only A/B rounds and the router
# profile, and packs bench/results and host details into /root/out.tar.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
cd /root
out=/root/out
mkdir -p "$out"

curl -fsSL https://get.docker.com | sh >/dev/null
snap install powershell --classic >/dev/null

mkdir -p rmx && tar -xf head.tar -C rmx
mkdir -p rmx/bench/rmx-app/public && tar -xf pkg.tar -C rmx/bench/rmx-app/public

{ lscpu; echo; lscpu -e; echo; free -m; echo; uname -a; docker version --format 'docker {{.Server.Version}}'; } > "$out/host.txt"

for ref in $AB_REFS; do
  mkdir -p "ref-$ref" && tar -xf "ref-$ref.tar" -C "ref-$ref"
  mkdir -p "ref-$ref/bench/rmx-app/public" && tar -xf pkg.tar -C "ref-$ref/bench/rmx-app/public"
  # The current Dockerfiles, so every build rebuilds the workspace from its own sources.
  cp rmx/bench/docker/*.Dockerfile rmx/bench/docker/*.dockerignore "ref-$ref/bench/docker/"
  docker build -q -t "rmxbench-rmx-$ref" -f "ref-$ref/bench/docker/rmx.Dockerfile" "ref-$ref"
done

cd rmx
echo "== full run against Remix"
pwsh bench/docker/run-docker.ps1 -Build -Out "docker-server-$SUFFIX.json" 2>&1 | tee "$out/docker-run-$SUFFIX.log"

for round in $(seq 1 "$ROUNDS"); do
  for image in rmxbench-rmx $(for ref in $AB_REFS; do echo "rmxbench-rmx-$ref"; done); do
    echo "== round $round: $image"
    pwsh bench/docker/run-docker.ps1 -Only rmx -RmxImage "$image" -Out "docker-ab-$SUFFIX-$image-$round.json" 2>&1 | grep 'req/s' | tee -a "$out/ab-$SUFFIX.log"
  done
done

echo "== router profile"
docker build -q -f bench/docker/profile.Dockerfile -t rmxbench-profile .
docker run --rm rmxbench-profile /router_profile | tee "$out/router-profile.txt"
docker run --rm -v "$PWD/bench/docker/profile-router.sh:/profile.sh:ro" rmxbench-profile sh /profile.sh | tee "$out/callgrind.txt"

cp bench/results/docker-*"$SUFFIX"*.json "$out/"
tar -cf /root/out.tar -C "$out" .
