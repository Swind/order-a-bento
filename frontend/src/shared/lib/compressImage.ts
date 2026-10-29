/** Longest side (px) of an uploaded photo (design §9.6). */
export const MAX_LONG_SIDE = 1568;
/** The backend rejects images above this size. */
export const MAX_UPLOAD_BYTES = 5 * 1024 * 1024;
const START_QUALITY = 0.85;
const MIN_QUALITY = 0.4;

/** Target size so that the long side is <= `maxSide`; never upscales. */
export function scaledSize(
  width: number,
  height: number,
  maxSide: number = MAX_LONG_SIDE,
): { width: number; height: number } {
  const long = Math.max(width, height);
  if (long <= maxSide) return { width, height };
  const ratio = maxSide / long;
  return {
    width: Math.max(1, Math.round(width * ratio)),
    height: Math.max(1, Math.round(height * ratio)),
  };
}

interface Decoded {
  source: CanvasImageSource;
  width: number;
  height: number;
  close: () => void;
}

async function decode(file: Blob): Promise<Decoded> {
  if (typeof createImageBitmap === "function") {
    try {
      // Respect EXIF orientation where the browser supports the option.
      const bmp = await createImageBitmap(file, { imageOrientation: "from-image" });
      return { source: bmp, width: bmp.width, height: bmp.height, close: () => bmp.close() };
    } catch {
      // fall back to <img>
    }
  }
  const url = URL.createObjectURL(file);
  try {
    const img = new Image();
    img.decoding = "async";
    img.src = url;
    await img.decode();
    return { source: img, width: img.naturalWidth, height: img.naturalHeight, close: () => {} };
  } finally {
    URL.revokeObjectURL(url);
  }
}

function toBlob(canvas: HTMLCanvasElement, quality: number): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (b) => (b ? resolve(b) : reject(new Error("toBlob failed"))),
      "image/jpeg",
      quality,
    );
  });
}

/**
 * Downscale a photo (long side <= 1568 px) and re-encode it as JPEG (quality 0.85,
 * lowered stepwise while above 5 MB). Throws if the image cannot be decoded.
 */
export async function compressImage(file: Blob): Promise<Blob> {
  const dec = await decode(file);
  try {
    const { width, height } = scaledSize(dec.width, dec.height);
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas unsupported");
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, width, height);
    ctx.drawImage(dec.source, 0, 0, width, height);
    let quality = START_QUALITY;
    let blob = await toBlob(canvas, quality);
    while (blob.size > MAX_UPLOAD_BYTES && quality > MIN_QUALITY) {
      quality = Math.max(MIN_QUALITY, quality - 0.15);
      blob = await toBlob(canvas, quality);
    }
    return blob;
  } finally {
    dec.close();
  }
}
