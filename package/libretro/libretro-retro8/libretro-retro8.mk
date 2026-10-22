################################################################################
#
# libretro-retro8
#
################################################################################

# Retro8 (PICO-8 compatible). Pinned upstream commit; second PICO-8 core
# after FAKE-08, chosen per game (package/libretro/README.md).
LIBRETRO_RETRO8_VERSION = ddc06a142398ee9755894b3f0bb17c8dc428151d
LIBRETRO_RETRO8_SITE = https://github.com/libretro/retro8.git
LIBRETRO_RETRO8_SITE_METHOD = git
LIBRETRO_RETRO8_LICENSE = GPL-3.0
LIBRETRO_RETRO8_LICENSE_FILES = LICENSE

define LIBRETRO_RETRO8_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(LIBRETRO_CORE_ENV) $(MAKE) -C $(@D) -f Makefile \
		$(LIBRETRO_CORE_MAKE_OPTS)
endef

define LIBRETRO_RETRO8_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/retro8_libretro.so \
		$(TARGET_DIR)/usr/lib/libretro/retro8_libretro.so
endef

$(eval $(generic-package))
