import Buffer from "bare-buffer";

export { Buffer };
export const INSPECT_MAX_BYTES = 50;
export const kMaxLength = Buffer.constants.MAX_LENGTH;
export const kStringMaxLength = Buffer.constants.MAX_STRING_LENGTH;

export default { Buffer, INSPECT_MAX_BYTES, kMaxLength, kStringMaxLength };
