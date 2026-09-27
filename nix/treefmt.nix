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
          # A SKILL.md opens with YAML frontmatter, which mdformat does not parse on its
          # own: it reads the leading `---` as a thematic break, rewrites it, and escapes
          # the underscores in the tool names below, leaving a file Claude Code sees as
          # having no frontmatter at all.
          #
          # This plugin teaches it the syntax, which beats excluding the files: an exclude
          # list has to name every skill tree, and the one here missed the second one the
          # day it was added.
          plugins = ps: [ ps.mdformat-frontmatter ];

          # Ordered lists are renumbered 1., 2., 3. rather than collapsed to all-1. Both
          # render alike, but a skill is read as source by the model that follows it, and
          # steps that all say 1. are worse to follow.
          settings.number = true;
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
