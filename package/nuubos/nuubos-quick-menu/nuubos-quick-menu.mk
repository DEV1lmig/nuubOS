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

$(eval $(cargo-package))
