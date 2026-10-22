################################################################################
# nuubos-quick-menu
################################################################################
NUUBOS_QUICK_MENU_VERSION = 0.1.0
NUUBOS_QUICK_MENU_SITE = $(BR2_EXTERNAL_NUUBOS_PATH)/package/nuubos/nuubos-quick-menu/src
NUUBOS_QUICK_MENU_SITE_METHOD = local
NUUBOS_QUICK_MENU_LICENSE = GPL-3.0-only, CC-BY-4.0
NUUBOS_QUICK_MENU_LICENSE_FILES = LICENSE ui/assets/icons/LICENSE.txt

define NUUBOS_QUICK_MENU_INSTALL_SESSION
	$(INSTALL) -D -m 0755 $(@D)/nuubos-quick-menu-session $(TARGET_DIR)/usr/bin/nuubos-quick-menu-session
endef
NUUBOS_QUICK_MENU_POST_INSTALL_TARGET_HOOKS += NUUBOS_QUICK_MENU_INSTALL_SESSION

NUUBOS_QUICK_MENU_DEPENDENCIES += host-python3
define NUUBOS_QUICK_MENU_INSTALL_NOTICES
	$(call NUUBOS_CARGO_NOTICES,nuubos-quick-menu,$(@D))
endef
NUUBOS_QUICK_MENU_POST_INSTALL_TARGET_HOOKS += NUUBOS_QUICK_MENU_INSTALL_NOTICES

# Cargo.toml's release profile (LTO, one codegen unit) is what we ship:
# Buildroot exports CARGO_PROFILE_RELEASE_LTO=false and CODEGEN_UNITS=16,
# and the environment beats Cargo.toml. The binary is read from the card
# at every boot (docs/dev/platform.md "Boot time").
NUUBOS_QUICK_MENU_CARGO_ENV = \
	CARGO_PROFILE_RELEASE_LTO="true" \
	CARGO_PROFILE_RELEASE_CODEGEN_UNITS="1"

$(eval $(cargo-package))
