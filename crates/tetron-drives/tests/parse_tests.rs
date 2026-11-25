//! Drive filtering on recorded `lsblk --json --bytes --list` output.

use tetron_drives::{clean_error, human_size, parse, parse_fstab, parse_mounted, parse_unlocked, Drive};

fn dev(name: &str, kind: &str, parent: Option<&str>, fs: Option<&str>, mounts: &[&str], extra: &str) -> String {
    let p = parent.map_or("null".to_string(), |p| format!("\"{p}\""));
    let f = fs.map_or("null".to_string(), |f| format!("\"{f}\""));
    let m = if mounts.is_empty() { "[null]".to_string() } else { format!("[{}]", mounts.iter().map(|m| format!("\"{m}\"")).collect::<Vec<_>>().join(",")) };
    format!(r#"{{"name":"{name}","path":"/dev/{name}","pkname":{p},"type":"{kind}","size":31000000000,"fstype":{f},"mountpoints":{m}{extra}}}"#)
}

fn lsblk(rows: &[String]) -> String {
    format!(r#"{{"blockdevices":[{}]}}"#, rows.join(","))
}

fn names(d: &[Drive]) -> Vec<&str> {
    d.iter().map(|x| x.name.as_str()).collect()
}

#[test]
fn installed_system_disk_hides_its_own_partitions_and_shows_a_usb_stick() {
    let j = lsblk(&[
        dev("nvme0n1", "disk", None, None, &[], r#","tran":"nvme","rm":false"#),
        dev("nvme0n1p1", "part", Some("nvme0n1"), Some("vfat"), &["/boot/efi"], r#","parttype":"c12a7328-f81f-11d2-ba4b-00a0c93ec93b""#),
        dev("nvme0n1p2", "part", Some("nvme0n1"), Some("ext4"), &["/"], ""),
        dev("nvme0n1p3", "part", Some("nvme0n1"), Some("swap"), &["[SWAP]"], ""),
        dev("nvme0n1p4", "part", Some("nvme0n1"), Some("ext4"), &["/srv/data"], ""),
        dev("nvme0n1p5", "part", Some("nvme0n1"), Some("ntfs"), &[], r#","label":"Windows""#),
        dev("sdb", "disk", None, None, &[], r#","tran":"usb","rm":true,"model":"Cruzer""#),
        dev("sdb1", "part", Some("sdb"), Some("exfat"), &[], r#","label":"STICK""#),
        dev("loop0", "loop", None, Some("squashfs"), &["/snap/core/1"], ""),
        dev("zram0", "disk", None, Some("swap"), &["[SWAP]"], ""),
    ]);
    let d = parse(&j, &["/srv/data".to_string()]);
    assert_eq!(names(&d), ["nvme0n1p5", "sdb1"], "system, fstab, swap, ESP, loop and zram are hidden");
    let stick = &d[1];
    assert_eq!(stick.title(), "STICK");
    assert_eq!(stick.disk, "/dev/sdb");
    assert!(stick.removable && !stick.mounted() && !stick.locked);
    assert_eq!(stick.bus.as_deref(), Some("usb"));
    assert_eq!(stick.model.as_deref(), Some("Cruzer"));
    assert!(!d[0].removable);
    assert_eq!(d[0].title(), "Windows");
}

#[test]
fn a_mounted_stick_reports_its_mount_point() {
    let j = lsblk(&[
        dev("sdc", "disk", None, None, &[], r#","hotplug":true"#),
        dev("sdc1", "part", Some("sdc"), Some("vfat"), &["/media/u/CARD"], ""),
    ]);
    let d = parse(&j, &[]);
    assert_eq!(d[0].mountpoint.as_deref(), Some(std::path::Path::new("/media/u/CARD")));
    assert!(d[0].removable, "hotplug counts as removable");
}

#[test]
fn the_live_stick_is_hidden_whole() {
    let j = lsblk(&[
        dev("sda", "disk", None, None, &[], r#","rm":true,"tran":"usb""#),
        dev("sda1", "part", Some("sda"), Some("iso9660"), &["/run/live/medium"], ""),
        dev("sda2", "part", Some("sda"), Some("vfat"), &[], ""),
        dev("sda3", "part", Some("sda"), Some("ext4"), &["/run/live/persistence/sda3"], r#","label":"persistence""#),
        dev("sr0", "rom", None, Some("iso9660"), &[], ""),
    ]);
    assert_eq!(names(&parse(&j, &[])), ["sr0"], "only a disc in the drive remains");
}

#[test]
fn a_whole_disk_filesystem_without_partitions_is_listed() {
    let j = lsblk(&[dev("sdd", "disk", None, Some("ext4"), &[], r#","label":"backup""#)]);
    let d = parse(&j, &[]);
    assert_eq!(d[0].dev, "/dev/sdd");
    assert_eq!(d[0].disk, "/dev/sdd");
}

#[test]
fn luks_locked_then_unlocked() {
    let locked = lsblk(&[
        dev("sde", "disk", None, None, &[], r#","rm":true"#),
        dev("sde1", "part", Some("sde"), Some("crypto_LUKS"), &[], ""),
    ]);
    let d = parse(&locked, &[]);
    assert_eq!(names(&d), ["sde1"]);
    assert!(d[0].locked && d[0].luks.is_none());

    let open = lsblk(&[
        dev("sde", "disk", None, None, &[], r#","rm":true"#),
        dev("sde1", "part", Some("sde"), Some("crypto_LUKS"), &[], ""),
        dev("dm-3", "crypt", Some("sde1"), Some("ext4"), &["/media/u/secret"], r#","label":"secret""#),
    ]);
    let d = parse(&open, &[]);
    assert_eq!(names(&d), ["dm-3"], "the opened volume replaces the LUKS partition");
    assert!(!d[0].locked);
    assert_eq!(d[0].luks.as_deref(), Some("/dev/sde1"));
    assert_eq!(d[0].disk, "/dev/sde", "eject powers off the stick under the volume");
    assert!(d[0].removable);
}

#[test]
fn lvm_members_hide_and_volumes_show() {
    let j = lsblk(&[
        dev("sdf", "disk", None, None, &[], ""),
        dev("sdf1", "part", Some("sdf"), Some("LVM2_member"), &[], ""),
        dev("vg-data", "lvm", Some("sdf1"), Some("xfs"), &[], ""),
        dev("sdf2", "part", Some("sdf"), None, &[], r#","parttype":"0x5""#),
    ]);
    assert_eq!(names(&parse(&j, &[])), ["vg-data"]);
}

#[test]
fn old_lsblk_booleans_and_mountpoint() {
    let j = r#"{"blockdevices":[
        {"name":"sdg","path":"/dev/sdg","pkname":null,"type":"disk","size":"1000","fstype":null,"rm":"1","hotplug":"0"},
        {"name":"sdg1","path":"/dev/sdg1","pkname":"sdg","type":"part","size":"1000","fstype":"vfat","mountpoint":"/media/u/X","rm":"1"}]}"#;
    let d = parse(j, &[]);
    assert_eq!(d.len(), 1);
    assert!(d[0].removable);
    assert_eq!(d[0].size, 1000);
    assert!(d[0].mounted());
}

#[test]
fn garbage_is_an_empty_list() {
    assert!(parse("not json", &[]).is_empty());
    assert!(parse("{}", &[]).is_empty());
}

#[test]
fn fstab_targets_skip_comments_swap_and_decode_spaces() {
    let t = parse_fstab("# c\nUUID=1 / ext4 defaults 0 1\nUUID=2 none swap sw 0 0\n/dev/sdb1 /mnt/my\\040disk ext4 defaults 0 2\n\n");
    assert_eq!(t, ["/", "/mnt/my disk"]);
}

#[test]
fn sizes_fit_a_narrow_column() {
    assert_eq!(human_size(0), "0B");
    assert_eq!(human_size(64 * 1024 * 1024), "64M");
    assert_eq!(human_size(31_000_000_000), "29G");
    assert_eq!(human_size(2_000_000_000_000), "1.8T");
    assert_eq!(human_size(512 * 1024 * 1024 * 1024), "512G");
}

#[test]
fn udisksctl_output_and_errors() {
    assert_eq!(parse_mounted("Mounted /dev/sdb1 at /media/u/STICK\n").unwrap(), std::path::PathBuf::from("/media/u/STICK"));
    assert_eq!(parse_mounted("Mounted /dev/sdb1 at /media/u/My Stick.\n").unwrap(), std::path::PathBuf::from("/media/u/My Stick"));
    assert_eq!(parse_unlocked("Unlocked /dev/sde1 as /dev/dm-3.\n").as_deref(), Some("/dev/dm-3"));
    assert_eq!(
        clean_error("Error mounting /dev/sdb1: GDBus.Error:org.freedesktop.UDisks2.Error.NotAuthorizedCanObtain: Not authorized to perform operation\n"),
        "Not authorized to perform operation"
    );
    assert_eq!(clean_error("Error unmounting /dev/sdb1: GDBus.Error:org.freedesktop.UDisks2.Error.DeviceBusy: Error unmounting /dev/sdb1: target is busy\n"), "Error unmounting /dev/sdb1: target is busy");
    assert_eq!(clean_error("plain failure"), "plain failure");
}

#[test]
fn users_of_finds_this_process_by_its_cwd() {
    let here = std::env::current_dir().unwrap();
    let users = tetron_drives::users_of(&here);
    assert!(users.iter().any(|(_, p)| *p == std::process::id()));
}

#[test]
fn watcher_reports_the_first_call_then_quiet() {
    let mut w = tetron_drives::Watcher::new();
    assert!(w.changed());
    assert!(!w.changed());
}

#[test]
fn find_by_path_kernel_name_or_another_name_of_the_node() {
    let all = parse(&lsblk(&[dev("sdb", "disk", None, None, &[], ""), dev("sdb1", "part", Some("sdb"), Some("vfat"), &[], "")]), &[]);
    assert_eq!(tetron_drives::find(&all, "/dev/sdb1").map(|d| d.name.as_str()), Some("sdb1"));
    assert_eq!(tetron_drives::find(&all, "sdb1").map(|d| d.name.as_str()), Some("sdb1"));
    assert!(tetron_drives::find(&all, "/dev/sdz9").is_none());
}
