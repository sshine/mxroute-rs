{ ... }:
{
  flake.overlays.default = final: _prev: {
    mxroute = final.callPackage ./_package.nix { };
    mxroute-mcp = final.callPackage ./_package.nix { crate = "mxroute-mcp"; };
  };
}
