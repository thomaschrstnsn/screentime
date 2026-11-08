# Spike: real setTimeLeft round-trip in the test VM

Throwaway asset for wayfinder ticket #7. Never merged.

- `../flake.nix`: the research sketch from #5. Host ports are 14222 (NATS) and 12222 (SSH), and root has an extra authorized key.
- `tk.sh`: helpers sourced inside the VM. `snap` calls getUserInformation; `stl <op> <secs>` calls setTimeLeft as `stcol`.

Run it with `nix build .#test-vm && ./result/bin/run-test-vm-vm`. The collector unit does not start on boot because of an ordering cycle (`timekpr.service` is `After=multi-user.target`). Start it with `systemctl start screentime-collector`.

The findings are in the resolution comment on #7.
