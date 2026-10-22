################################################################################
#
# nuubui-home
#
################################################################################

NUUBUI_HOME_VERSION = 0.1.0
NUUBUI_HOME_SITE = $(BR2_EXTERNAL_NUUBOS_PATH)/package/nuubos/nuubui-home/src
NUUBUI_HOME_SITE_METHOD = local
NUUBUI_HOME_LICENSE = GPL-3.0-only, CC-BY-4.0
NUUBUI_HOME_LICENSE_FILES = LICENSE ui/assets/icons/LICENSE.txt ui/assets/icons/ATTRIBUTION.txt

# i-slint-core with the nuubOS patches (patches/packages/i-slint-core), used
# through [patch.crates-io] in Cargo.toml: the pristine crate comes from the
# offline cargo cache.
NUUBUI_HOME_SLINT_CORE_CRATE = $(firstword $(wildcard $(DL_DIR)/br-cargo-home/registry/cache/*/i-slint-core-1.18.1.crate))
define NUUBUI_HOME_VENDOR_SLINT_CORE
	test -f "$(NUUBUI_HOME_SLINT_CORE_CRATE)"
	rm -rf $(@D)/vendor/i-slint-core
	mkdir -p $(@D)/vendor/i-slint-core
	tar -xzf $(NUUBUI_HOME_SLINT_CORE_CRATE) -C $(@D)/vendor/i-slint-core --strip-components=1
	$(APPLY_PATCHES) $(@D)/vendor/i-slint-core $(BR2_EXTERNAL_NUUBOS_PATH)/patches/packages/i-slint-core \*.patch
endef
NUUBUI_HOME_POST_RSYNC_HOOKS += NUUBUI_HOME_VENDOR_SLINT_CORE

NUUBUI_HOME_DEPENDENCIES += host-python3
define NUUBUI_HOME_INSTALL_NOTICES
	$(call NUUBOS_CARGO_NOTICES,nuubui-home,$(@D))
endef
NUUBUI_HOME_POST_INSTALL_TARGET_HOOKS += NUUBUI_HOME_INSTALL_NOTICES

# Cargo.toml's release profile (LTO, one codegen unit) is what we ship:
# Buildroot exports CARGO_PROFILE_RELEASE_LTO=false and CODEGEN_UNITS=16,
# and the environment beats Cargo.toml. The binary is read from the card
# at every boot (docs/dev/platform.md "Boot time").
NUUBUI_HOME_CARGO_ENV = \
	CARGO_PROFILE_RELEASE_LTO="true" \
	CARGO_PROFILE_RELEASE_CODEGEN_UNITS="1"

$(eval $(cargo-package))
