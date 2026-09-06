{ ... }:
{
  flake.overlays.default = final: _prev: {
    mxroute = final.callPackage ./_package.nix { };
  };
}
