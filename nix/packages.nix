# `packages.default` stays the library: it is what this repository publishes, and what
# `checks.mxroute` names. The server is reached by name, which is also what an MCP client
# configuration would spell out.
{ ... }:
{
  perSystem =
    { pkgs, lib, ... }:
    rec {
      checks.mxroute = packages.default;
      checks.mxroute-mcp = packages.mxroute-mcp;

      packages.default = pkgs.callPackage ./_package.nix { };
      packages.mxroute-mcp = pkgs.callPackage ./_package.nix { crate = "mxroute-mcp"; };

      # The only runnable thing here, so it is the default app even though it is not the
      # default package. `nix run github:sshine/mxroute-rs` starts the MCP server on stdio.
      apps.mxroute-mcp = {
        type = "app";
        program = lib.getExe packages.mxroute-mcp;
      };
      apps.default = apps.mxroute-mcp;
    };
}
