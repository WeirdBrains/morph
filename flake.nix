{
  description = "Morph: distributed web browser with local AI";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      # The prebuilt release (packaging/nix/release.json): `morph` with the
      # desktop entry and icons, `morph-standalone` with only the binary.
      packages = forAll (pkgs: rec {
        morph = pkgs.callPackage ./packaging/nix/package.nix { };
        morph-standalone = pkgs.callPackage ./packaging/nix/package.nix { desktop = false; };
        default = morph;
      });

      apps = forAll (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.morph}/bin/morph";
          meta.description = "Morph, the web browser";
        };
      });
    };
}
