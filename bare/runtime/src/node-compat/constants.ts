export const O_RDONLY = 0;
export const O_WRONLY = 1;
export const O_RDWR = 2;
export const O_CREAT = 64;
export const O_EXCL = 128;
export const O_NOCTTY = 256;
export const O_TRUNC = 512;
export const O_APPEND = 1024;
export const O_DIRECTORY = 65536;
export const O_NOATIME = 262144;
export const O_NOFOLLOW = 131072;
export const O_SYNC = 1052672;
export const O_DSYNC = 4096;
export const O_DIRECT = 16384;
export const O_NONBLOCK = 2048;
export const F_OK = 0;
export const R_OK = 4;
export const W_OK = 2;
export const X_OK = 1;
export const S_IFMT = 61440;
export const S_IFREG = 32768;
export const S_IFDIR = 16384;
export const S_IFCHR = 8192;
export const S_IFBLK = 24576;
export const S_IFIFO = 4096;
export const S_IFLNK = 40960;
export const S_IFSOCK = 49152;
export const S_IRWXU = 448;
export const S_IRUSR = 256;
export const S_IWUSR = 128;
export const S_IXUSR = 64;
export const S_IRWXG = 56;
export const S_IRGRP = 32;
export const S_IWGRP = 16;
export const S_IXGRP = 8;
export const S_IRWXO = 7;
export const S_IROTH = 4;
export const S_IWOTH = 2;
export const S_IXOTH = 1;
export const SIGINT = 2;
export const SIGTERM = 15;

export const signals = Object.freeze({
  SIGHUP: 1,
  SIGINT,
  SIGQUIT: 3,
  SIGILL: 4,
  SIGTRAP: 5,
  SIGABRT: 6,
  SIGBUS: 7,
  SIGFPE: 8,
  SIGKILL: 9,
  SIGUSR1: 10,
  SIGSEGV: 11,
  SIGUSR2: 12,
  SIGPIPE: 13,
  SIGALRM: 14,
  SIGTERM,
});

export const errno = Object.freeze({});
export const priority = Object.freeze({
  PRIORITY_LOW: 19,
  PRIORITY_BELOW_NORMAL: 10,
  PRIORITY_NORMAL: 0,
  PRIORITY_ABOVE_NORMAL: -7,
  PRIORITY_HIGH: -14,
  PRIORITY_HIGHEST: -20,
});

const constants = Object.freeze({
  F_OK,
  O_APPEND,
  O_CREAT,
  O_DIRECT,
  O_DIRECTORY,
  O_DSYNC,
  O_EXCL,
  O_NOATIME,
  O_NOCTTY,
  O_NOFOLLOW,
  O_NONBLOCK,
  O_RDONLY,
  O_RDWR,
  O_SYNC,
  O_TRUNC,
  O_WRONLY,
  R_OK,
  S_IFBLK,
  S_IFCHR,
  S_IFDIR,
  S_IFIFO,
  S_IFLNK,
  S_IFMT,
  S_IFREG,
  S_IFSOCK,
  S_IRGRP,
  S_IROTH,
  S_IRUSR,
  S_IRWXG,
  S_IRWXO,
  S_IRWXU,
  S_IWGRP,
  S_IWOTH,
  S_IWUSR,
  S_IXGRP,
  S_IXOTH,
  S_IXUSR,
  SIGINT,
  SIGTERM,
  W_OK,
  X_OK,
});

export default constants;
