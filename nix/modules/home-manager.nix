# nix/modules/home-manager.nix — auto-generated from lava-core.caixa.lisp
{ config, lib, pkgs, ... }:
let cfg = config.programs.lava-core; in {
  options.programs.lava-core = {
    enable = lib.mkEnableOption "lava-core";
    package = lib.mkOption { type = lib.types.package; default = pkgs.lava-core or null; };
  };
  config = lib.mkIf cfg.enable { home.packages = [ cfg.package ]; };
}
