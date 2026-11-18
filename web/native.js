// Where the native client is downloaded from, per system (the links under the start window): none
// until a release names them. web/build.sh writes this file from DBOTA_NATIVE_DOWNLOADS (a JSON
// object of `windows`, `linux` and `macos` addresses) when it is set; without it the page shows no
// download links.
export const NATIVE_CLIENT = null;
