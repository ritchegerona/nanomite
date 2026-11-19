# USB-X Rescue Kit — Recovery Playbook
**Drive:** USB-X (exFAT)  
**Created:** 2024  
**Purpose:** Corrupted Windows recovery, data backup, file rescue, system repair

---

## QUICK START

| Scenario | Go to |
|----------|-------|
| Windows won't boot / corrupted system files | Boot into RESCUE-ISO (SystemRescue) |
| Recover deleted/lost files from Windows drive | `SCRIPTS/file-recover.sh` (Mac) or PhotoRec in RESCUE-ISO |
| Backup Windows user data before reinstall | `SCRIPTS/backup-windows-data.sh` |
| Scan for malware on Windows drive | ClamAV in RESCUE-ISO or Windows Defender Offline |
| Reset forgotten Windows admin password | `chntpw` in RESCUE-ISO |

---

## FOLDER LAYOUT

```
/RESCUE-ISO/      -> Bootable ISO images (SystemRescue, Windows ISO)
/TOOLS-WINDOWS/   -> Portable .exe tools to run on working Windows
/TOOLS-MAC/       -> Mac recovery utilities
/SCRIPTS/         -> Shell scripts for backup & recovery
/BACKUP-WORKSPACE/-> Output destination for recovered files
/DOCS/            -> This playbook + cheat sheets
/BOOT/            -> Bootloader files (if making USB bootable)
```

---

## SCENARIO 1: WINDOWS WON'T BOOT (Corrupted System)

### Step A: Boot from USB-X Rescue ISO
1. Download **SystemRescue** ISO (system-rescue-linux.org) and place in `/RESCUE-ISO/`
2. Make USB-X bootable:
   - **Windows:** Use Rufus or Ventoy to flash the ISO to USB-X
   - **Mac/Linux:** `dd if=systemrescue.iso of=/dev/diskX bs=4M status=progress`
3. Boot target PC from USB-X (press F12/F2/Esc/Del at startup to choose boot device)

### Step B: Repair Windows from Rescue Environment
Once booted into SystemRescue:

```bash
# 1. Identify Windows partition
lsblk -f
# Look for NTFS partition (usually /dev/sda2, /dev/sda3, etc.)

# 2. Mount Windows partition
ntfs-3g /dev/sda2 /mnt/windows

# 3. Fix NTFS errors
ntfsfix /dev/sda2

# 4. Backup user data before any destructive repair
mkdir -p /mnt/usb-backup
mount /dev/sdb2 /mnt/usb-backup   # USB-X DATA partition
cp -av /mnt/windows/Users/* /mnt/usb-backup/RECOVERED-$(date +%Y%m%d)/

# 5. Repair boot sector (if bootmgr/BCD is missing)
mount --bind /dev /mnt/windows/dev
mount --bind /proc /mnt/windows/proc
mount --bind /sys /mnt/windows/sys
chroot /mnt/windows
bootrec /fixmbr
bootrec /fixboot
bootrec /rebuildbcd
exit

# 6. Unmount and reboot
umount -R /mnt/windows
reboot
```

---

## SCENARIO 2: RECOVER DELETED OR LOST FILES

### Option A: PhotoRec (in SystemRescue)
```bash
photorec /dev/sda2
# Select partition -> file types -> destination (/mnt/usb-backup)
# Recovered files go to recup_dir.1, recup_dir.2, etc.
```

### Option B: TestDisk (recover lost partitions)
```bash
testdisk /dev/sda
# Analyze -> Quick Search -> Deeper Search -> Write partition table
```

### Option C: Mac-side recovery script
If you're on Mac and the Windows drive is externally connected:

```bash
bash /Volumes/USB-X/SCRIPTS/file-recover.sh /dev/diskX /Volumes/USB-X/BACKUP-WORKSPACE
```

---

## SCENARIO 3: BACKUP WINDOWS DATA BEFORE REINSTALL

Use the provided script on a **working Mac** with the Windows drive connected externally:

```bash
bash /Volumes/USB-X/SCRIPTS/backup-windows-data.sh
```

This copies:
- `C:\Users\*\Documents`
- `C:\Users\*\Desktop`
- `C:\Users\*\Pictures`
- `C:\Users\*\Downloads`
- `C:\Users\*\Favorites`
- `C:\Users\*\AppData\Roaming\Microsoft\Outlook` (if present)

Output goes to `/Volumes/USB-X/BACKUP-WORKSPACE/`.

---

## SCENARIO 4: MALWARE SCAN ON WINDOWS DRIVE

Boot SystemRescue, then:

```bash
# Update ClamAV definitions
freshclam

# Scan Windows partition
clamscan -r --infected --remove=no /mnt/windows/ > /mnt/usb-backup/scan-results.txt

# Or use Windows Defender Offline (download from Microsoft, put in RESCUE-ISO/)
```

---

## PORTABLE TOOLS (TOOLS-WINDOWS/)

Download these `.exe` files and place in `/TOOLS-WINDOWS/`. Run them on any working Windows PC:

| Tool | Purpose | Download |
|------|---------|----------|
| Rufus | Make bootable USB | rufus.ie |
| Ventoy | Multi-boot USB creator | ventoy.net |
| TestDisk/PhotoRec | Partition & file recovery | cgsecurity.org |
| Recuva | Deleted file recovery | ccleaner.com/recuva |
| CrystalDiskInfo | Drive health check | crystalmark.info |
| BlueLife Key Freeze | Lock keyboard/mouse during repair | bluelife-soft.com |
| ProduKey | Recover Windows/Product keys | nirsoft.net |
| NirSoft USBDeview | USB device history | nirsoft.net |

---

## COMMAND CHEAT SHEET

```bash
# List all disks (Mac)
diskutil list

# Unmount a disk (Mac)
diskutil unmountDisk /dev/diskX

# Write ISO to USB (Mac/Linux)
sudo dd if=systemrescue.iso of=/dev/rdiskX bs=4m status=progress

# Check disk health
smartctl -a /dev/sda

# Clone a failing drive (ddrescue)
ddrescue -f -n /dev/sda /mnt/usb-backup/image.img /mnt/usb-backup/mapfile.log

# Mount NTFS read-write (Linux rescue)
ntfs-3g -o remove_hiberfile /dev/sda2 /mnt/windows
```

---

## SAFETY RULES

1. **Never write to the corrupted drive** until you've backed up data.
2. **Always verify backups** before formatting or reinstalling.
3. **Work from a read-only mount** if possible: `mount -o ro /dev/sda2 /mnt/windows`
4. **Label recovered folders** with date: `RECOVERED-20241109`
5. **Keep USB-X write-protected** when scanning for malware (physical switch if available)

---

## SUPPORT

If stuck: check `/DOCS/` for this playbook, or boot SystemRescue and run `help` / `man <command>`.

**Good luck — recovery is possible in most cases.** 🛠️