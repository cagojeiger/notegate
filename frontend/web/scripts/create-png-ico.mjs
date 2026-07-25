import { readFile, writeFile } from "node:fs/promises";

const [output, ...inputPaths] = process.argv.slice(2);

if (!output || inputPaths.length === 0) {
  throw new Error("usage: create-png-ico.mjs <output.ico> <input.png>...");
}

const images = await Promise.all(inputPaths.map(async (path) => {
  const data = await readFile(path);
  const { width, height } = pngDimensions(data, path);
  if (width > 256 || height > 256) {
    throw new Error(`ICO PNG dimensions must not exceed 256px: ${path}`);
  }
  return { data, width, height };
}));

const directorySize = 6 + images.length * 16;
const header = Buffer.alloc(directorySize);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(images.length, 4);

let offset = directorySize;
for (const [index, image] of images.entries()) {
  const entry = 6 + index * 16;
  header.writeUInt8(image.width === 256 ? 0 : image.width, entry);
  header.writeUInt8(image.height === 256 ? 0 : image.height, entry + 1);
  header.writeUInt8(0, entry + 2);
  header.writeUInt8(0, entry + 3);
  header.writeUInt16LE(1, entry + 4);
  header.writeUInt16LE(32, entry + 6);
  header.writeUInt32LE(image.data.length, entry + 8);
  header.writeUInt32LE(offset, entry + 12);
  offset += image.data.length;
}

await writeFile(output, Buffer.concat([header, ...images.map(({ data }) => data)]));

function pngDimensions(data, path) {
  const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  if (data.length < 24 || !data.subarray(0, 8).equals(signature) || data.toString("ascii", 12, 16) !== "IHDR") {
    throw new Error(`Not a PNG file: ${path}`);
  }
  return {
    width: data.readUInt32BE(16),
    height: data.readUInt32BE(20)
  };
}
