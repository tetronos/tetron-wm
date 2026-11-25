#!/usr/bin/env python3
"""`tetron-wm drives` (the Files Drives code, crate tetron-drives) against real drives in a VM, as a
user without sudo, from a serial console login (not an active seat session, like kmscon and ssh).
Usage: tests/vm/drives-vm.py ISO TETRONOS_STORAGE_DEB TETRON_WM_BINARY   (a TetronOS live ISO; needs kvm, qemu)
Drives: an exFAT USB stick, a second USB stick made LUKS in the VM, an ext4 SATA disk.
Work dir: target/vm-drives (V=1 prints every command's output)."""
import os, re, shutil, socket, subprocess, sys, time

iso, deb, binary = (os.path.abspath(a) for a in sys.argv[1:4])
here = os.path.dirname(os.path.abspath(__file__))
w = os.path.join(here, "..", "..", "target", "vm-drives"); os.makedirs(w, exist_ok=True)
stamp = int(time.time())
usb, usb2, sata, carry, sock = (f"{w}/{n}-{stamp}" for n in ("usb.raw", "usb2.raw", "sata.raw", "carry.raw", "ser.sock"))

def part(img, size, mkfs):
    """One partition at 1 MiB, its filesystem made on an offset image (no root needed)."""
    subprocess.run(["truncate", "-s", size, img], check=True)
    subprocess.run(["sfdisk", "-q", img], input=b"label: gpt\n,\n", check=True)
    if not mkfs: return
    p = f"{img}.p1"; subprocess.run(["truncate", "-s", str(os.path.getsize(img) - 2 * 1024 * 1024), p], check=True)
    subprocess.run([*mkfs, p], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["dd", f"if={p}", f"of={img}", "bs=1M", "seek=1", "conv=notrunc", "status=none"], check=True); os.remove(p)

part(usb, "64M", ["mkfs.exfat", "-L", "STICK"])
part(usb2, "64M", None)
part(sata, "64M", ["mke2fs", "-q", "-t", "ext4", "-L", "data"])
d = f"{w}/carry-{stamp}"; os.makedirs(d); shutil.copy(deb, d); shutil.copy(binary, f"{d}/tetron-wm")
subprocess.run(["mke2fs", "-q", "-t", "ext4", "-L", "carry", "-d", d, carry, "64M"], check=True); shutil.rmtree(d)

q = subprocess.Popen(["qemu-system-x86_64", "-enable-kvm", "-cpu", "host", "-m", "2048", "-smp", "2",
    "-drive", f"file={iso},media=cdrom,readonly=on", "-boot", "d",
    "-device", "ahci,id=ahci", "-drive", f"file={sata},format=raw,if=none,id=sata", "-device", "ide-hd,drive=sata,bus=ahci.0,serial=SATADISK",
    "-device", "qemu-xhci",
    "-drive", f"file={usb},format=raw,if=none,id=usb", "-device", "usb-storage,drive=usb,removable=on,serial=USBSTICK",
    "-drive", f"file={usb2},format=raw,if=none,id=usb2", "-device", "usb-storage,drive=usb2,removable=on,serial=USBVAULT",
    "-drive", f"file={carry},format=raw,if=virtio",
    "-display", "none", "-nic", "user,model=virtio-net-pci", "-serial", f"unix:{sock},server=on,wait=off"])
fails = []
def check(name, ok):
    print(("PASS " if ok else "FAIL ") + name, flush=True)
    if not ok: fails.append(name)
try:
    for _ in range(50):
        if os.path.exists(sock): break
        time.sleep(0.2)
    s = socket.socket(socket.AF_UNIX); s.connect(sock); s.settimeout(1)
    buf = ""
    def expect(rx, timeout):
        global buf
        end = time.time() + timeout
        while time.time() < end:
            m = re.search(rx, buf)
            if m:
                out, buf = buf[:m.end()], buf[m.end():]; return out
            try: buf += s.recv(65536).decode(errors="replace")
            except socket.timeout: pass
        raise SystemExit(f"timeout waiting for {rx!r}; tail: {buf[-400:]!r}")
    def send(t): s.sendall(t.encode())
    expect(r"login:", 240); send("tetronos\n"); expect(r"Password:", 20); send("live\n"); expect(r"\$ ", 60)
    n = [0]
    def run(cmd, timeout=60):
        n[0] += 1; tag = f"M{n[0]}X"
        send(f"{cmd}; echo {tag}$?\n")
        out = expect(tag + r"\d+", timeout)
        rc = int(re.search(tag + r"(\d+)", out).group(1)); expect(r"\$ ", 10)
        if os.environ.get("V"): print("---", cmd[:80], "\n", out[-700:], flush=True)
        return rc, out.replace("\r", "")
    def dev(serial):
        _, out = run(f"lsblk -dno NAME,SERIAL | awk '$2==\"{serial}\"{{print \"/dev/\"$1\"1\"}}'")
        m = re.search(r"(/dev/[a-z]+1)\n", out); return m.group(1) if m else "/dev/missing"
    run("sudo mount -o ro /dev/vda /mnt && sudo cp /mnt/*.deb /run/ && sudo install -m 0755 /mnt/tetron-wm /usr/local/bin/tetron-wm && sudo umount /mnt")
    rc, out = run("sudo apt-get update -qq 2>&1 | tail -1; sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /run/tetronos-storage_*.deb cryptsetup-bin 2>&1 | tail -2", 900)
    check("tetronos-storage and cryptsetup-bin install", rc == 0)
    stick, vault, disk = dev("USBSTICK"), dev("USBVAULT"), dev("SATADISK")
    # The second stick becomes LUKS (root only here, to set the test up): ext4 "vault" inside.
    rc, _ = run(f"printf pw | sudo cryptsetup luksFormat -q --key-file - {vault} && printf pw | sudo cryptsetup open --key-file - {vault} tv && sudo mkfs.ext4 -q -L vault /dev/mapper/tv && sudo cryptsetup close tv", 120)
    check("LUKS stick prepared", rc == 0)
    rc, out = run("tetron-wm drives")
    check("list shows the stick, the SATA disk and the locked stick, all not mounted",
          re.search(rf"{stick}\s+STICK\s+\S+\s+exfat\s+rm\s+-", out) is not None
          and re.search(rf"{disk}\s+data\s+\S+\s+ext4\s+-\s+-", out) is not None
          and re.search(rf"{vault}\s+\S+\s+\S+\s+crypto_LUKS\s+rm\s+locked", out) is not None)
    check("the live medium is not listed", "iso9660" not in out and "/run/live" not in out)
    rc, out = run(f"tetron-wm drives mount {stick}", 30)
    check("mount the stick without sudo", rc == 0 and "/media/tetronos/STICK" in out)
    rc, _ = run("echo hi > /media/tetronos/STICK/t && grep -q hi /media/tetronos/STICK/t"); check("write to it", rc == 0)
    run("(cd /media/tetronos/STICK && exec sleep 300) & sleep 1")
    rc, out = run(f"tetron-wm drives unmount {stick}", 30)
    check("unmount of a busy stick names the program", rc != 0 and "in use by sleep" in out)
    run("kill %1; wait 2>/dev/null; sleep 1")
    rc, out = run(f"tetron-wm drives unmount {stick}", 30); check("unmount once free", rc == 0)
    rc, out = run(f"printf pw | tetron-wm drives unlock {vault}", 30)
    m = re.search(r"(/dev/dm-\d+)", out)
    check("unlock the LUKS stick without sudo", rc == 0 and m is not None)
    clear = m.group(1) if m else "/dev/missing"
    rc, out = run("tetron-wm drives")
    # lsblk names it /dev/mapper/luks-UUID; udisksctl printed /dev/dm-N: the same device.
    check("unlocked: the volume replaces the LUKS partition", re.search(r"\s+vault\s+\S+\s+ext4\s+rm\s+-", out) is not None and vault not in out)
    rc, out = run(f"tetron-wm drives mount {clear}", 30); check("mount the unlocked volume", rc == 0 and "/media/tetronos/vault" in out)
    rc, out = run(f"tetron-wm drives eject {clear}", 60); check("eject the LUKS stick (unmount, lock, power off)", rc == 0)
    rc, out = run("sleep 2; tetron-wm drives")
    check("the ejected stick is gone from the list", "vault" not in out and vault not in out)
    rc, out = run(f"tetron-wm drives mount {disk}", 30); check("mount the internal SATA disk", rc == 0 and "/media/tetronos/data" in out)
    rc, out = run(f"tetron-wm drives eject {stick}", 60); check("eject the exFAT stick", rc == 0)
    rc, out = run("sleep 2; tetron-wm drives")
    check("after eject only the SATA disk remains, mounted", "STICK" not in out and re.search(rf"{disk}\s+data\s.*/media/tetronos/data", out) is not None)
    rc, out = run(f"tetron-wm drives unmount {disk}", 30); check("unmount the SATA disk", rc == 0)
    rc, out = run("tetron-wm drives mount /dev/sr0 2>&1", 30); check("the live medium is refused", rc != 0)
    send("sudo systemctl poweroff\n")
    try: q.wait(timeout=120)
    except subprocess.TimeoutExpired: q.kill(); q.wait()
finally:
    if q.poll() is None: q.kill(); q.wait()
print(f"{'FAIL' if fails else 'PASS'}: {len(fails)} failed", flush=True)
sys.exit(1 if fails else 0)
