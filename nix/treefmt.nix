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
        programs.mdformat = {
          enable = true;
          # A SKILL.md opens with YAML frontmatter, which mdformat does not recognise: it
          # reads the leading `---` as a thematic break, rewrites it, and escapes the
          # underscores in the tool names below it. The result is a file Claude Code reads
          # as having no frontmatter at all.
          # Both skill trees: the one the plugin ships and the one this repository uses
          # on itself. A pattern matching either would have to be a glob mdformat and
          # treefmt agree on, and naming them is shorter than finding out.
          excludes = [
            "plugin/skills/*/SKILL.md"
            ".claude/skills/*/SKILL.md"
          ];
        };
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
