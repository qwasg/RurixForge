/* G8.3 M83 — 薄 C API 实现,包真实 `basis_universal`(BinomialLLC 1.16.4)。
 * 设计案 §3.6。本文件 = Rurix 自有代码(MIT OR Apache-2.0)。
 *
 * 确定性钳制(RXS-0334):
 *   1. job_pool(1) —— 上游要求 m_pJob_pool 非空;1 = 仅调用线程,零额外线程;
 *      并 m_multithreading=false,使 RDO/frontend 不走并行路径。
 *   2. 固定档位:ETC1S m_compression_level 与 m_quality_level 硬编码;
 *      UASTC m_pack_uastc_flags = cPackUASTCLevelDefault;禁 RDO。
 *   3. 禁 zstd / 任意 supercompression:KTX2_SS_NONE + 编译期 BASISD_SUPPORT_KTX2_ZSTD=0。
 *   4. 禁 OpenCL(BASISU_SUPPORT_OPENCL=0 + m_use_opencl=false)。
 *   5. 禁 mip 生成(单 mip)、禁 y_flip、禁 stats/status 输出(不写 stdout)。
 */
#include "rurix_basis_wrap.h"

#include <cstring>
#include <cstdlib>
#include <limits>
#include <memory>
#include <mutex>
#include <string>

#include "../vendor/basis_universal/encoder/basisu_comp.h"
#include "../vendor/basis_universal/transcoder/basisu_transcoder.h"
#include "../vendor/basis_universal/encoder/jpgd.h"
#include "../vendor/basis_universal/encoder/pvpngreader.h"
#define MINIZ_HEADER_FILE_ONLY
#define MINIZ_NO_ZLIB_COMPATIBLE_NAMES
#include "../vendor/basis_universal/encoder/basisu_miniz.h"

namespace {

/* 版本串 = "basis_universal/<tag>+g<commit12>",与 VENDOR.md / SBOM.md 字面全等。
 * 由 vendor_manifest.json 的 tag/commit 派生,写死在此以便 FFI 零分配返回。 */
const char kRurixBasisVersion[] = "basis_universal/1.16.4+g900e40fb5d25";

std::once_flag g_init_once;

void init_library() {
  std::call_once(g_init_once, [] {
    /* use_opencl=false, opencl_force_serialization=false */
    basisu::basisu_encoder_init(false, false);
    basist::basisu_transcoder_init();
  });
}

int publish(const basisu::uint8_vec &src, RurixBasisBuf *out) {
  if (src.empty()) {
    return 20;
  }
  uint8_t *p = new (std::nothrow) uint8_t[src.size()];
  if (!p) {
    return 21;
  }
  std::memcpy(p, src.data(), src.size());
  out->data = p;
  out->len = src.size();
  return 0;
}

uint32_t read_be32(const uint8_t *p) {
  return (uint32_t(p[0]) << 24) | (uint32_t(p[1]) << 16) |
         (uint32_t(p[2]) << 8) | uint32_t(p[3]);
}

bool raster_size(uint32_t width, uint32_t height, size_t &bytes) {
  // Both pinned decoders support dimensions at most 32768. Their internal
  // scanline/Adam7 arithmetic and Rust slice construction need a signed-size
  // bound, so reject impossible images before any pixel allocation.
  const uint64_t n = uint64_t(width) * uint64_t(height) * 4;
  if (!width || !height || width > 32768 || height > 32768 ||
      n > uint64_t(std::numeric_limits<int32_t>::max()) ||
      n > uint64_t(std::numeric_limits<ptrdiff_t>::max())) {
    return false;
  }
  bytes = static_cast<size_t>(n);
  return true;
}

bool png_envelope(const uint8_t *data, size_t len) {
  const uint8_t signature[] = {137, 80, 78, 71, 13, 10, 26, 10};
  // The vendor reader asserts on inputs shorter than its MIN_PNG_SIZE (46).
  if (len < 57 || std::memcmp(data, signature, sizeof(signature))) return false;
  bool idat = false;
  for (size_t offset = 8; offset < len;) {
    if (len - offset < 12) return false;
    const size_t chunk_len = read_be32(data + offset);
    if (chunk_len > len - offset - 12) return false;
    const uint8_t *type = data + offset + 4;
    if (offset == 8 && (chunk_len != 13 || std::memcmp(type, "IHDR", 4))) return false;
    if (offset != 8 && !std::memcmp(type, "IHDR", 4)) return false;
    // Validate the entire envelope, including IEND/trailing chunks which the
    // scanline decoder need not consume after it has produced the final row.
    if (uint32_t(buminiz::mz_crc32(0, type, chunk_len + 4)) !=
        read_be32(type + 4 + chunk_len)) return false;
    if (!std::memcmp(type, "IDAT", 4)) idat = true;
    offset += chunk_len + 12;
    if (!std::memcmp(type, "IEND", 4)) return idat && !chunk_len && offset == len;
  }
  return false;
}

bool jpeg_envelope(const uint8_t *data, size_t len) {
  if (len < 4 || data[0] != 0xff || data[1] != 0xd8) return false;
  bool in_scan = false, has_scan = false;
  size_t offset = 2;
  while (offset < len) {
    if (in_scan && data[offset] != 0xff) { ++offset; continue; }
    if (data[offset++] != 0xff) return false;
    while (offset < len && data[offset] == 0xff) ++offset;
    if (offset == len) return false;
    const uint8_t marker = data[offset++];
    if (in_scan && (marker == 0 || (marker >= 0xd0 && marker <= 0xd7))) continue;
    in_scan = false;
    // A real EOI is mandatory: jpgd pads truncated memory streams with EOI.
    if (marker == 0xd9) return has_scan;
    if (!marker || marker == 0xd8 || marker == 0x01 ||
        (marker >= 0xd0 && marker <= 0xd7) || len - offset < 2) return false;
    const size_t chunk_len = (size_t(data[offset]) << 8) | data[offset + 1];
    if (chunk_len < 2 || chunk_len > len - offset) return false;
    offset += chunk_len;
    if (marker == 0xda) { in_scan = true; has_scan = true; }
  }
  return false;
}

int decode_raster(const uint8_t *data, size_t len, int format,
                  RurixBasisBuf *out, uint32_t *out_width, uint32_t *out_height) {
  uint32_t width = 0, height = 0;
  size_t bytes = 0;
  std::unique_ptr<uint8_t[]> output;
  if (format == RURIX_BASIS_RASTER_PNG) {
    if (!png_envelope(data, len)) return 30;
    pv_png::png_info info;
    if (!pv_png::get_png_info(data, len, info)) return 31;
    if (!raster_size(info.m_width, info.m_height, bytes)) return 32;
    uint32_t channels = 0;
    // The underlying loader owns malloc storage, independent of the new[]
    // output that crosses the ABI. Avoid basisu::load_png's unchecked pointer.
    std::unique_ptr<void, decltype(&std::free)> decoded(
        pv_png::load_png(data, len, 4, width, height, channels), &std::free);
    if (!decoded || width != info.m_width || height != info.m_height) return 33;
    output.reset(new (std::nothrow) uint8_t[bytes]);
    if (!output) return 21;
    std::memcpy(output.get(), decoded.get(), bytes);
  } else if (format == RURIX_BASIS_RASTER_JPEG) {
    if (!jpeg_envelope(data, len)) return 34;
    jpgd::jpeg_decoder_mem_stream stream(data, static_cast<uint32_t>(len));
    jpgd::jpeg_decoder decoder(&stream, jpgd::jpeg_decoder::cFlagLinearChromaFiltering);
    if (decoder.get_error_code() != jpgd::JPGD_SUCCESS) return 35;
    width = static_cast<uint32_t>(decoder.get_width());
    height = static_cast<uint32_t>(decoder.get_height());
    if (!raster_size(width, height, bytes)) return 32;
    const int components = decoder.get_num_components();
    if ((components != 1 && components != 3) ||
        decoder.begin_decoding() != jpgd::JPGD_SUCCESS) return 36;
    // Use jpgd's scanline API to avoid the high-level helper's signed int
    // width*height*4 allocation and retain RAII ownership on every failure.
    output.reset(new (std::nothrow) uint8_t[bytes]);
    if (!output) return 21;
    for (uint32_t y = 0; y < height; ++y) {
      const void *scan = nullptr;
      jpgd::uint scan_len = 0;
      if (decoder.decode(&scan, &scan_len) != jpgd::JPGD_SUCCESS || !scan ||
          scan_len < width * (components == 1 ? 1u : 4u)) return 37;
      uint8_t *dst = output.get() + size_t(y) * width * 4;
      if (components == 3) {
        std::memcpy(dst, scan, size_t(width) * 4);
      } else {
        const uint8_t *src = static_cast<const uint8_t *>(scan);
        for (uint32_t x = 0; x < width; ++x) {
          dst[4 * x] = dst[4 * x + 1] = dst[4 * x + 2] = src[x];
          dst[4 * x + 3] = 255;
        }
      }
    }
    const void *scan = nullptr;
    jpgd::uint scan_len = 0;
    if (decoder.decode(&scan, &scan_len) != jpgd::JPGD_DONE) return 37;
  } else {
    return 3;
  }
  // Publish only once all fallible work is complete.
  out->len = bytes;
  out->data = output.release();
  if (out_width) *out_width = width;
  if (out_height) *out_height = height;
  return 0;
}

/* 确定性 encoder 参数基线。swizzle_rg != 0 时把 XY normal 流铺成
 * R→RGB / G→A,使 ETC1S/UASTC 的 alpha slice 携带 Y,BC5 腿 transcode 后
 * X=R、Y=G 语义成立(上游 cTFBC5_RG 取 color.r 与 alpha)。 */
void apply_determinism_clamps(basisu::basis_compressor_params &p,
                              basisu::job_pool *pool, int swizzle_rg) {
  p.m_pJob_pool = pool;
  p.m_multithreading = false;
  p.m_use_opencl = false;

  /* 不读磁盘、不写磁盘、不打印 */
  p.m_read_source_images = false;
  p.m_write_output_basis_files = false;
  p.m_status_output = false;
  p.m_compute_stats = false;
  p.m_print_stats = false;
  p.m_debug = false;
  p.m_debug_images = false;

  /* 单 mip、无翻转 */
  p.m_mip_gen = false;
  p.m_y_flip = false;

  /* 禁 UASTC RDO(引入额外启发式与并行路径) */
  p.m_rdo_uastc = false;

  /* 禁任何 KTX2 supercompression */
  p.m_ktx2_uastc_supercompression = basist::KTX2_SS_NONE;

  if (swizzle_rg) {
    /* R,R,R,G —— alpha 通道承载 Y */
    p.m_swizzle[0] = 0;
    p.m_swizzle[1] = 0;
    p.m_swizzle[2] = 0;
    p.m_swizzle[3] = 1;
    p.m_force_alpha = true;
  } else {
    p.m_swizzle[0] = 0;
    p.m_swizzle[1] = 1;
    p.m_swizzle[2] = 2;
    p.m_swizzle[3] = 3;
  }
}

}  // namespace

extern "C" {

const char *rurix_basis_version(void) { return kRurixBasisVersion; }

void rurix_basis_buf_free(RurixBasisBuf *buf) {
  if (!buf) {
    return;
  }
  delete[] buf->data;
  buf->data = nullptr;
  buf->len = 0;
}

int rurix_basis_decode_raster_rgba8(const uint8_t *data, size_t len, int format,
                                    RurixBasisBuf *out, uint32_t *out_width,
                                    uint32_t *out_height) {
  if (out_width) *out_width = 0;
  if (out_height) *out_height = 0;
  if (!out) return 1;
  out->data = nullptr;
  out->len = 0;
  if (!data || !len || len > size_t(std::numeric_limits<int32_t>::max())) return 2;
  try {
    return decode_raster(data, len, format, out, out_width, out_height);
  } catch (...) {
    // No C++ exception may cross the C ABI; local unique_ptr and decoder
    // destructors release all intermediate allocations during unwinding.
    return 38;
  }
}

int rurix_basis_encode_container(const uint8_t *rgba, uint32_t width,
                                 uint32_t height, int mode, int swizzle_rg,
                                 RurixBasisBuf *out) {
  if (!out) {
    return 1;
  }
  out->data = nullptr;
  out->len = 0;
  if (!rgba || width == 0 || height == 0) {
    return 2;
  }
  if (mode != RURIX_BASIS_MODE_UASTC_KTX2 && mode != RURIX_BASIS_MODE_ETC1S_BASIS) {
    return 3;
  }

  init_library();

  basisu::job_pool pool(1);
  basisu::basis_compressor_params p;
  apply_determinism_clamps(p, &pool, swizzle_rg);

  if (mode == RURIX_BASIS_MODE_UASTC_KTX2) {
    p.m_uastc = true;
    p.m_create_ktx2_file = true;
    p.m_pack_uastc_flags = basisu::cPackUASTCLevelDefault;
  } else {
    p.m_uastc = false;
    p.m_create_ktx2_file = false;
    /* 固定 ETC1S 档位(禁 -1 自适应,保证跨机同参) */
    p.m_compression_level = 2;
    p.m_quality_level = 128;
  }

  {
    basisu::image img;
    img.init(rgba, width, height, 4);
    p.m_source_images.push_back(img);
  }

  basisu::basis_compressor comp;
  if (!comp.init(p)) {
    return 10;
  }
  basisu::basis_compressor::error_code ec = comp.process();
  if (ec != basisu::basis_compressor::cECSuccess) {
    return 100 + static_cast<int>(ec);
  }

  const basisu::uint8_vec &result = (mode == RURIX_BASIS_MODE_UASTC_KTX2)
                                        ? comp.get_output_ktx2_file()
                                        : comp.get_output_basis_file();
  return publish(result, out);
}

int rurix_basis_transcode(const uint8_t *data, size_t len, int src_kind,
                          int target, RurixBasisBuf *out, uint32_t *out_width,
                          uint32_t *out_height) {
  return rurix_basis_transcode_level(data, len, src_kind, target, 0, out,
                                     out_width, out_height);
}

int rurix_basis_transcode_level(const uint8_t *data, size_t len, int src_kind,
                                int target, uint32_t level, RurixBasisBuf *out,
                                uint32_t *out_width, uint32_t *out_height) {
  if (!out) {
    return 1;
  }
  out->data = nullptr;
  out->len = 0;
  if (!data || len == 0 || len > 0xFFFFFFFFu) {
    return 2;
  }

  switch (target) {
    case RURIX_BASIS_TF_BC4_R:
    case RURIX_BASIS_TF_BC5_RG:
    case RURIX_BASIS_TF_BC7_RGBA:
    case RURIX_BASIS_TF_ASTC_4x4:
      break;
    default:
      return 3;
  }

  init_library();

  const basist::transcoder_texture_format fmt =
      static_cast<basist::transcoder_texture_format>(target);
  const uint32_t bytes_per_block = basist::basis_get_bytes_per_block_or_pixel(fmt);
  const uint32_t data_size = static_cast<uint32_t>(len);

  uint32_t w = 0, h = 0, blocks_x = 0, blocks_y = 0, total_blocks = 0;

  if (src_kind == RURIX_BASIS_SRC_KTX2) {
    basist::ktx2_transcoder t;
    if (!t.init(data, data_size)) {
      return 11;
    }
    if (t.get_levels() == 0) {
      return 12;
    }
    if (level >= t.get_levels()) {
      return 17;
    }
    if (!t.start_transcoding()) {
      return 13;
    }
    basist::ktx2_image_level_info info;
    if (!t.get_image_level_info(info, level, 0, 0)) {
      return 14;
    }
    w = info.m_orig_width;
    h = info.m_orig_height;
    blocks_x = info.m_num_blocks_x;
    blocks_y = info.m_num_blocks_y;
    total_blocks = blocks_x * blocks_y;
    if (total_blocks == 0) {
      return 15;
    }
    basisu::uint8_vec buf(static_cast<size_t>(total_blocks) * bytes_per_block);
    if (!t.transcode_image_level(level, 0, 0, buf.data(), total_blocks, fmt, 0, 0, 0)) {
      return 16;
    }
    if (out_width) *out_width = w;
    if (out_height) *out_height = h;
    return publish(buf, out);
  }

  if (src_kind != RURIX_BASIS_SRC_BASIS) {
    return 4;
  }
  if (level != 0) {
    /* 在树 .basis 产物恒单级(m_mip_gen=false 钳制);level 参数面仅 KTX2。 */
    return 5;
  }

  basist::basisu_transcoder t;
  if (!t.validate_header(data, data_size)) {
    return 21;
  }
  if (t.get_total_images(data, data_size) == 0) {
    return 22;
  }
  if (!t.start_transcoding(data, data_size)) {
    return 23;
  }
  basist::basisu_image_level_info info;
  if (!t.get_image_level_info(data, data_size, info, 0, 0)) {
    return 24;
  }
  w = info.m_orig_width;
  h = info.m_orig_height;
  total_blocks = info.m_total_blocks;
  if (total_blocks == 0) {
    return 25;
  }
  basisu::uint8_vec buf(static_cast<size_t>(total_blocks) * bytes_per_block);
  if (!t.transcode_image_level(data, data_size, 0, 0, buf.data(), total_blocks, fmt,
                               0, 0, nullptr, 0)) {
    return 26;
  }
  if (out_width) *out_width = w;
  if (out_height) *out_height = h;
  return publish(buf, out);
}

}  /* extern "C" */
