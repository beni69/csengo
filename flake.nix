{
  description = "Remote-controlled, schedule-based audio player";

  outputs = { self, nixpkgs, flake-utils }: flake-utils.lib.eachDefaultSystem (system: {
    devShells.default = with nixpkgs.legacyPackages.${system}; mkShell {
      buildInputs = [
        pkg-config
        alsa-lib
        sqlite-interactive
      ];

      shellHook = ''
        export LD_LIBRARY_PATH="${alsa-lib}/lib:$LD_LIBRARY_PATH"
      '';
    };
  });
}
