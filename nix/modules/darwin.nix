# nix/modules/darwin.nix — auto-generated from lava-core.caixa.lisp
{ config, lib, pkgs, ... }:
let cfg = config.services.lava-core; in {
  options.services.lava-core = {
    enable = lib.mkEnableOption "lava-core";
    package = lib.mkOption { type = lib.types.package; default = pkgs.lava-core or null; };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
