################################################################################
#
# libretro-yabasanshiro
#
################################################################################

# YabaSanshiro (Sega Saturn), aarch64 dynarec + GLES3. Pinned upstream commit; part of the
# default H700 core matrix (package/libretro/README.md). Bundled as
# YabaSanshiro-nuubOS (yabasanshiro_nuubos) with H700 performance patches
# (docs/dev/emulation.md "Saturn, PICO-8, overscan").
LIBRETRO_YABASANSHIRO_VERSION = 09ed8e5b2e97e7a848ea2514545c34c7b809e399
LIBRETRO_YABASANSHIRO_SITE = $(call github,libretro,yabause,$(LIBRETRO_YABASANSHIRO_VERSION))
LIBRETRO_YABASANSHIRO_LICENSE = GPL-2.0
LIBRETRO_YABASANSHIRO_LICENSE_FILES = LICENSE
LIBRETRO_YABASANSHIRO_DEPENDENCIES = libgles libretro-core-info

define LIBRETRO_YABASANSHIRO_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(LIBRETRO_CORE_ENV) $(MAKE) -C $(@D)/yabause/src/libretro -f Makefile \
		$(LIBRETRO_CORE_MAKE_OPTS) LD="$(TARGET_CXX)" platform=arm64_cortex_a53_gles3
endef

define LIBRETRO_YABASANSHIRO_INSTALL_TARGET_CMDS
	$(call LIBRETRO_NUUBOS_CORE_INSTALL,yabasanshiro,yabasanshiro_nuubos,$(@D)/yabause/src/libretro/yabasanshiro_libretro.so)
endef

$(eval $(generic-package))
