# GNS3 feature coverage

Internal map: which GNS3 features Isothrum covers, which it covers partly, which it plans, and
which it leaves out. Built from GNS3's documented feature set; the docs site returned only its
navigation when fetched, so each row should be checked against docs.gns3.com before we claim it
publicly.

Status: **Covered** (works today in Isothrum or Isoloom) · **Partial** (part of it, or a different
mechanism) · **Planned** (on the roadmap) · **Out** (not planned; reason given).

## Nodes

| GNS3 feature | Status | Isothrum / Isoloom mapping |
|---|---|---|
| Docker container nodes | Covered | `kind: container`, one image per node |
| QEMU / full VM nodes | Covered | `kind: vm`, Vagrant, Proxmox and cloud targets |
| VirtualBox / VMware nodes | Partial | Through Vagrant providers, not as separate node types |
| VPCS (simple PC) | Planned | A small container image with the same commands |
| Ethernet switch | Partial | A cable network is a switch segment; no explicit switch node yet |
| Ethernet hub | Out | Hubs are switches with no MAC learning; nothing to model |
| Cloud node (bridge to host) | Partial | Hybrid target uses macvlan on the VM host; no explicit cloud node |
| NAT node | Partial | `internet` flag and gateway machines; no explicit NAT node |
| Cisco IOS / Dynamips / IOU router images | Out | Proprietary images; use FRR or another container as the router |
| Frame Relay and ATM switches | Out | Legacy WAN technologies |
| Router as FRR container | Planned | Needs the routing decision (see below) |

## Links

| GNS3 feature | Status | Isothrum / Isoloom mapping |
|---|---|---|
| Point-to-point cabling between ports | Covered | Each cable is its own /29 network (`isothrum-core`) |
| Named ports (interfaces) | Covered | Free-text port names on each endpoint |
| Link filters: latency, jitter, loss, bandwidth | Planned | `tc netem` in a sidecar, generated per target |
| Start / stop / suspend a link | Planned | Needs the per-node runner |
| Packet capture on a link | Planned | Capture container on the link's network |
| Routing between links | Open | Real routers (recommended) or Isoloom's `reach` rules |

## Consoles and runtime

| GNS3 feature | Status | Isothrum / Isoloom mapping |
|---|---|---|
| Telnet console to a node | Planned | `docker exec` or a web terminal |
| VNC / SPICE console | Planned | Browser console for VM nodes |
| Start / stop / suspend / reload a node | Planned | Per-node runner, beyond `isoloom run` / `down` |
| Auto-start on open | Planned | Runner setting |
| Snapshots of a node | Planned | Docker commit for containers, VM snapshot for VMs |

## Projects and editing

| GNS3 feature | Status | Isothrum / Isoloom mapping |
|---|---|---|
| Save and reopen a project | Covered | `isoloom.yml` is the project file; the editor writes it |
| Import an existing config | Partial | `isoloom import compose / vagrant / terraform` exists; no `.gns3` import |
| Export a project | Covered | The editor downloads `isoloom.yml` and the generated bundle |
| Node templates / appliances | Planned | Saved node presets in the editor |
| Drawing tools: notes, shapes, labels | Planned | Editor-only; nothing in the spec |
| Topology summary | Partial | `isoloom validate`, `targets`, `resources` |
| Validation before running | Covered | `isoloom validate` problems shown in the editor |

## Remote and scale

| GNS3 feature | Status | Isothrum / Isoloom mapping |
|---|---|---|
| Remote GNS3 server | Out for v1 | Proxmox and cloud targets cover remote hosts |
| GNS3 VM (the bundled Linux VM) | Out | Vagrant and Proxmox targets do this job |
| Multi-user / shared labs | Out for v1 | Revisit with the hosted option |
| Clustering | Out for v1 | Proxmox |

## Beyond GNS3

Isothrum/Isoloom features GNS3 does not have, which should lead the marketing:

- One spec runs on Docker, VMs, Proxmox and the clouds.
- Checks: black-box scripts that prove the behavior on every target.
- `reach` rules that declare which networks may talk, verified by checks.
- The spec is a plain text file that works in Git and CI.

## Open decisions that block rows

1. **Routing** (blocks router, link filters on routed paths, capture): real routers or `reach` rules.
2. **Runner** (blocks start/stop, consoles, snapshots, capture): the runner is the next big piece.
