"""
glTF 2.0 Binary (GLB) Reader, Writer, and Accessor Utilities
"""
import struct
import json
import numpy as np

DTYPE_MAP = {
    5120: np.int8,
    5121: np.uint8,
    5122: np.int16,
    5123: np.uint16,
    5125: np.uint32,
    5126: np.float32,
}

SHAPE_MAP = {
    "SCALAR": 1,
    "VEC2": 2,
    "VEC3": 3,
    "VEC4": 4,
    "MAT4": 16,
}

REVERSE_DTYPE = {
    np.dtype("int8"): 5120,
    np.dtype("uint8"): 5121,
    np.dtype("int16"): 5122,
    np.dtype("uint16"): 5123,
    np.dtype("uint32"): 5125,
    np.dtype("float32"): 5126,
}


def load_glb(file_path):
    with open(file_path, "rb") as f:
        data = f.read()
    if len(data) < 12 or data[:4] != b"glTF":
        raise ValueError(f"Not a valid GLB file: {file_path}")
    version, total_len = struct.unpack_from("<II", data, 4)
    if version != 2:
        raise ValueError(f"Unsupported glTF version: {version}")

    offset = 12
    json_chunk = None
    bin_chunk = b""
    while offset < len(data):
        chunk_len, chunk_type = struct.unpack_from("<II", data, offset)
        offset += 8
        chunk_data = data[offset : offset + chunk_len]
        offset += chunk_len
        if offset % 4 != 0:
            offset += 4 - (offset % 4)

        if chunk_type == 0x4E4F534A:  # "JSON"
            json_chunk = json.loads(chunk_data.decode("utf-8").strip())
        elif chunk_type == 0x004E4942:  # "BIN\x00"
            bin_chunk = chunk_data

    if json_chunk is None:
        raise ValueError("GLB has no JSON chunk")
    return json_chunk, bytearray(bin_chunk)


def save_glb(output_path, json_data, bin_data):
    json_bytes = json.dumps(json_data, separators=(",", ":")).encode("utf-8")
    json_pad = (4 - (len(json_bytes) % 4)) % 4
    json_bytes += b" " * json_pad

    bin_pad = (4 - (len(bin_data) % 4)) % 4
    bin_bytes = bytes(bin_data) + (b"\x00" * bin_pad)

    total_len = 12 + (8 + len(json_bytes)) + (8 + len(bin_bytes))
    header = struct.pack("<4sII", b"glTF", 2, total_len)
    json_header = struct.pack("<II", len(json_bytes), 0x4E4F534A)
    bin_header = struct.pack("<II", len(bin_bytes), 0x004E4942)

    with open(output_path, "wb") as f:
        f.write(header)
        f.write(json_header)
        f.write(json_bytes)
        f.write(bin_header)
        f.write(bin_bytes)


def get_accessor_data(gltf, bin_data, accessor_idx):
    acc = gltf["accessors"][accessor_idx]
    count = acc["count"]
    comp_type = acc["componentType"]
    type_str = acc["type"]
    num_comp = SHAPE_MAP[type_str]
    dtype = DTYPE_MAP[comp_type]

    bv_idx = acc.get("bufferView")
    if bv_idx is None:
        return np.zeros((count, num_comp) if num_comp > 1 else (count,), dtype=dtype)

    bv = gltf["bufferViews"][bv_idx]
    bv_offset = bv.get("byteOffset", 0)
    acc_offset = acc.get("byteOffset", 0)
    byte_stride = bv.get("byteStride")

    start = bv_offset + acc_offset
    item_size = np.dtype(dtype).itemsize * num_comp

    if byte_stride is None or byte_stride == item_size:
        total_bytes = count * item_size
        raw = bin_data[start : start + total_bytes]
        arr = np.frombuffer(raw, dtype=dtype)
        if num_comp > 1:
            arr = arr.reshape((count, num_comp))
        return arr.copy()
    else:
        out = np.zeros((count, num_comp) if num_comp > 1 else (count,), dtype=dtype)
        for i in range(count):
            offset = start + i * byte_stride
            chunk = bin_data[offset : offset + item_size]
            item = np.frombuffer(chunk, dtype=dtype)
            if num_comp > 1:
                out[i] = item
            else:
                out[i] = item[0]
        return out


def add_buffer_data(bin_data, raw_bytes):
    # Ensure 4-byte alignment before appending
    align = (4 - (len(bin_data) % 4)) % 4
    if align > 0:
        bin_data.extend(b"\x00" * align)
    offset = len(bin_data)
    bin_data.extend(raw_bytes)
    return offset


def add_buffer_view(gltf, bin_data, raw_bytes, target=None):
    """
    Appends raw_bytes to bin_data (with 4-byte alignment) and adds a bufferView to gltf.
    Returns the new bufferView index.
    """
    offset = add_buffer_data(bin_data, raw_bytes)
    if "bufferViews" not in gltf:
        gltf["bufferViews"] = []
    bv_idx = len(gltf["bufferViews"])
    bv = {
        "buffer": 0,
        "byteOffset": offset,
        "byteLength": len(raw_bytes),
    }
    if target is not None:
        bv["target"] = target
    gltf["bufferViews"].append(bv)
    return bv_idx


def add_accessor(gltf, bin_data, array, type_str, target=None):
    """
    Appends array to bin_data, adds bufferView and accessor to gltf dict.
    Returns accessor index.
    """
    arr = np.ascontiguousarray(array)
    raw = arr.tobytes()
    bv_idx = add_buffer_view(gltf, bin_data, raw, target=target)

    if "accessors" not in gltf:
        gltf["accessors"] = []

    comp_type = REVERSE_DTYPE[arr.dtype]
    count = len(arr)
    acc = {
        "bufferView": bv_idx,
        "byteOffset": 0,
        "componentType": comp_type,
        "count": count,
        "type": type_str,
    }

    if type_str == "SCALAR":
        acc["min"] = [float(np.min(arr))]
        acc["max"] = [float(np.max(arr))]
    elif type_str in ("VEC2", "VEC3", "VEC4"):
        acc["min"] = [float(x) for x in np.min(arr, axis=0)]
        acc["max"] = [float(x) for x in np.max(arr, axis=0)]

    acc_idx = len(gltf["accessors"])
    gltf["accessors"].append(acc)
    return acc_idx
