# USB-X Rescue Playbook

## Scenario A: Windows PC won't boot, need files off the drive
1. Boot target PC from USB-X (SystemRescue).
2. Choose default boot option.
3. Open terminal. List drives: `lsblk`
4. Mount Windows partition read-only:
   ```bash
   mkdir -p /mnt/win
   mount -o ro /dev/sdXY /mnt/win   # replace sdXY with Windows partition
   ```
5. Copy user data to USB-X:
   ```bash
   cp -rv /mnt/win/Users/*/Documents /mnt/win/Users/*/Desktop /mnt/usb/backup/
   ```

## Scenario B: Corrupted NTFS / "drive needs formatting" error
1. On SystemRescue:
   ```bash
   ntfsfix /dev/sdXY
   ```
2. If ntfsfix fails, use TestDisk:
   ```bash
   testdisk /dev/sdX
   ```
   - Select partition table type (usually Intel/PC)
   - Analyze → Quick Search → Deeper Search
   - Write recovered partition table
3. Recover deleted/lost files with PhotoRec:
   ```bash
   photorec /dev/sdXY
   ```

## Scenario C: Virus suspected on Windows drive
1. Boot SystemRescue from USB-X.
2. Update ClamAV: `freshclam`
3. Scan mounted Windows drive:
   ```bash
   clamscan -r --infected --remove=no /mnt/win
   ```

## Scenario D: Reset forgotten Windows local password (own PC only)
1. Boot SystemRescue.
2. Use chntpw:
   ```bash
   chntpw -u Username /mnt/win/Windows/System32/config/SAM
   ```
3. Choose password clear/edit, write changes, reboot.

## Scenario E: Mac-side backup (Windows drive connected to Mac)
```bash
sudo bash /Volumes/USB-X/SCRIPTS/backup-windows-data.sh /Volumes/WindowsDrive /Volumes/USB-X/BACKUP-WORKSPACE
```

## Safety rules
- Never write to the corrupted drive until data is copied off.
- Always mount read-only (`-o ro`) first.
- Verify copied files before wiping or formatting anything.
