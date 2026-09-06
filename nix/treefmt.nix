{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];
  perSystem =
    { pkgs, ... }:
    let
      rust-toolchain = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
    in
    {
      treefmt = {
        projectRootFile = "flake.nix";

        # A pinned copy of the upstream spec, kept byte-identical so a refetch shows a
        # real diff rather than a reformat.
        settings.global.excludes = [ "openapi.yaml" ];

        programs.nixfmt.enable = true;
        programs.rustfmt = {
          enable = true;
          package = rust-toolchain;
        };
        programs.mdformat.enable = true;
        programs.taplo.enable = true;
        programs.yamlfmt = {
          enable = true;
          # Blank lines separate the sections of a workflow file, and collapsing them
          # turns `on:`, `permissions:` and `jobs:` into one wall of keys.
          settings.formatter.retain_line_breaks_single = true;
        };
      };
    };
}
