from functools import lru_cache
import json
import subprocess


@lru_cache(maxsize=1)
def physical_topology():
    raw = subprocess.check_output(["lscpu", "--parse=CPU,CORE,SOCKET"], text=True)
    groups = {}
    for line in raw.splitlines():
        if not line or line.startswith("#"):
            continue
        logical, core, socket = map(int, line.split(","))
        groups.setdefault((socket, core), []).append(logical)
    return groups


def cpu_snapshot(cpu):
    raw = subprocess.check_output(
        ["mpstat", "-P", "ALL", "-o", "JSON", "1", "1"], text=True
    )
    data = json.loads(raw)["sysstat"]["hosts"][0]["statistics"][0]["cpu-load"]
    cores = [item for item in data if str(item["cpu"]) != "all"]
    idle = {int(item["cpu"]): item["idle"] for item in cores}
    groups = physical_topology()
    selected = next(siblings for siblings in groups.values() if cpu in siblings)
    idle_cores = sum(
        all(idle.get(sibling, 0) > 50 for sibling in siblings)
        for siblings in groups.values()
    )
    return raw, idle_cores >= 3 and all(
        idle.get(sibling, 0) >= 90 for sibling in selected
    )
