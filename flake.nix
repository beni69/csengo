{
  description = "Remote-controlled, schedule-based audio player";

  outputs = { self, nixpkgs, flake-utils }: flake-utils.lib.eachDefaultSystem (system: {
    devShells.default = with nixpkgs.legacyPackages.${system}; mkShell {
      buildInputs = [
        pkg-config
        alsa-lib
        sqlite-interactive
      ];
    };
  });
}
