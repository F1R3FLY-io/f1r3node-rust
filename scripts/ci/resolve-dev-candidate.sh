#!/usr/bin/env bash
# Resolve immutable candidate identities for a dev commit from the CI-published images.
#
# CI publishes every dev push to Docker Hub under two tag forms. The ":dev" tag is a
# mutable pointer. The "dev-<git describe>" tag is immutable and names the commit.
# A soak candidate must cite the immutable tag and the digests below, never ":dev".
#
# Usage: resolve-dev-candidate.sh <dev-commit-sha> <harness-revision> > candidates.json
#
# Requires: curl, python3, docker with buildx. Needs network and a Docker daemon.
# Reads only. It pulls images and copies one file out. It never runs a node.
set -euo pipefail
SHA="$1"; HARNESS="$2"; SHORT="${SHA:0:8}"
IMG=docker.io/f1r3flyindustries/f1r3fly-rust
REPO=f1r3flyindustries/f1r3fly-rust
TAG=$(curl -s "https://hub.docker.com/v2/repositories/$REPO/tags?name=g${SHORT}&page_size=50" \
  | python3 -c "import json,sys; ts=[t['name'] for t in json.load(sys.stdin)['results'] if t['name'].startswith('dev-') and not t['name'].endswith(('-amd64','-arm64'))]; print(ts[0] if ts else '')")
[[ -n "$TAG" ]] || { echo "no immutable dev tag for $SHORT on Docker Hub" >&2; exit 2; }
INDEX=$(docker buildx imagetools inspect "$IMG:$TAG" --raw)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
python3 - "$SHA" "$HARNESS" "$TAG" "$INDEX" "$WORK" <<'PY'
import json,sys,subprocess,hashlib,os
sha,harness,tag,index,work=sys.argv[1:6]
img="docker.io/f1r3flyindustries/f1r3fly-rust"
idx=json.loads(index); out=[]
for m in idx["manifests"]:
    p=m["platform"]; plat=f'{p["os"]}/{p["architecture"]}'; arch=p["architecture"]
    ref=f'{img}@{m["digest"]}'
    subprocess.run(["docker","pull","-q","--platform",plat,ref],check=True,capture_output=True)
    cfg=subprocess.run(["docker","image","inspect","--format","{{.Id}}",ref],check=True,capture_output=True,text=True).stdout.strip()
    cid=subprocess.run(["docker","create","--platform",plat,ref],check=True,capture_output=True,text=True).stdout.strip()
    dst=os.path.join(work,f"node-{arch}")
    subprocess.run(["docker","cp",f"{cid}:/opt/docker/bin/node",dst],check=True,capture_output=True)
    subprocess.run(["docker","rm",cid],check=True,capture_output=True)
    h=hashlib.sha256(open(dst,"rb").read()).hexdigest()
    out.append({"candidate_id":f"dev-{arch}","node_revision":sha,"platform":plat,"harness_revision":harness,
        "image_tag":f"{img.split('/',1)[1]}:{tag}","image_digest":m["digest"],"image_digest_kind":"oci-platform-manifest",
        "image_config_digest":cfg,"node_binary_digest":f"sha256:{h}","node_binary_path":"opt/docker/bin/node",
        "node_binary_bytes":os.path.getsize(dst),"workload_configuration_digest":None,
        "workload_configuration_status":"blocked-unimplemented-profile-request","admission":"blocked"})
json.dump({"schema_version":1,"resolved_from":"docker-hub","index_digest_note":"the :dev pointer is mutable; the git-describe tag is immutable","candidates":out},sys.stdout,indent=2)
PY
