#!/usr/bin/env python3
"""Inspect CDFV2/OLE2 encrypted Excel file for embedded strings/hints."""

import struct
import sys

def read_cdfv2(filepath):
    with open(filepath, 'rb') as f:
        data = f.read()
    
    # OLE2 header
    magic = data[:8]
    print(f"Magic: {magic.hex()} ({magic})")
    
    # Sector size (at offset 0x1C for version 3, or 0x20)
    sector_size_shift = struct.unpack('<H', data[0x1C:0x1E])[0]
    sector_size = 1 << sector_size_shift
    print(f"Sector size: {sector_size} (2^{sector_size_shift})")
    
    # Mini sector size
    mini_sector_shift = struct.unpack('<H', data[0x20:0x22])[0]
    mini_sector_size = 1 << mini_sector_shift
    print(f"Mini sector size: {mini_sector_size}")
    
    # Directory entries start at offset 0x40 for standard
    dir_offset = 0x40
    dir_entry_size = 128
    
    # Read directory entries
    num_sectors_in_dir = struct.unpack('<I', data[0x2C:0x30])[0]
    print(f"Directory sectors: {num_sectors_in_dir}")
    
    # Parse directory entries
    entries = []
    for i in range(num_sectors_in_dir * (sector_size // dir_entry_size)):
        offset = dir_offset + i * dir_entry_size
        if offset + dir_entry_size > len(data):
            break
        entry = data[offset:offset + dir_entry_size]
        
        # Name (64 bytes, UTF-16LE)
        name_raw = entry[:64]
        name_len = struct.unpack('<I', entry[0x40:0x44])[0]
        try:
            name = name_raw[:name_len].decode('utf-16-le', errors='replace')
        except:
            name = name_raw.decode('utf-16-le', errors='replace')
        
        # Object type (byte 0x40)
        obj_type = entry[0x40]
        
        # Color flag (byte 0x42)
        color = entry[0x42]
        
        # Left sibling (0x44)
        left = struct.unpack('<i', entry[0x44:0x48])[0]
        right = struct.unpack('<i', entry[0x48:0x4C])[0]
        child = struct.unpack('<i', entry[0x4C:0x50])[0]
        
        # CLSID (0x50, 16 bytes)
        clsid = entry[0x50:0x60]
        
        # State bits (0x60)
        state = struct.unpack('<I', entry[0x60:0x64])[0]
        
        # Start sector (0x64)
        start_sector = struct.unpack('<i', entry[0x64:0x68])[0]
        
        # Size (0x68)
        size = struct.unpack('<I', entry[0x68:0x6C])[0]
        
        if name.strip('\x00'):
            type_name = {1: "STORAGE", 2: "STREAM", 5: "ROOT"}.get(obj_type, f"TYPE_{obj_type}")
            print(f"Entry {i}: name='{name}' type={type_name} obj_type={obj_type} start={start_sector} size={size} state={state}")
            entries.append((name, obj_type, start_sector, size, i))
    
    return data, sector_size, entries

def read_stream(data, sector_size, start_sector, size):
    """Read a stream from the OLE2 file."""
    result = b''
    sector = start_sector
    remaining = size
    while sector >= 0 and remaining > 0:
        offset = sector * sector_size + 512  # +512 for header
        chunk = data[offset:offset + min(sector_size, remaining)]
        result += chunk
        remaining -= len(chunk)
        # Read next sector from FAT (we don't have FAT parsed, just try sequential)
        sector += 1  # simplification
        if remaining > 0:
            break
    return result

if __name__ == '__main__':
    filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"
    data, sector_size, entries = read_cdfv2(filepath)
    
    print(f"\nTotal file size: {len(data)} bytes")
    
    # Search for any ASCII strings that might be password hints
    print("\n--- Searching for ASCII strings ---")
    i = 0
    while i < len(data):
        if 32 <= data[i] < 127:
            start = i
            while i < len(data) and 32 <= data[i] < 127:
                i += 1
            s = data[start:i].decode('ascii')
            if len(s) >= 4:
                print(f"  offset 0x{start:06X}: {s}")
        else:
            i += 1
    
    print("\n--- Searching for UTF-16LE strings ---")
    i = 0
    while i < len(data) - 2:
        if data[i+1] == 0 and 32 <= data[i] < 127:
            start = i
            while i < len(data) - 1 and data[i+1] == 0 and 32 <= data[i] < 127:
                i += 2
            s = data[start:i].decode('utf-16-le', errors='replace')
            if len(s) >= 3:
                print(f"  offset 0x{start:06X}: {s}")
        i += 1