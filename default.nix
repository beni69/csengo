with import <nixpkgs> { };

stdenv.mkDerivation {
  name = "csengo";
  buildInputs = [
    pkg-config
    alsa-lib
    sqlite
  ];
}
