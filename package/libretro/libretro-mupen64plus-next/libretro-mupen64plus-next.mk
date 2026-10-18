################################################################################
#
# libretro-mupen64plus-next
#
################################################################################

# Mupen64Plus-Next (Nintendo 64), aarch64 dynarec + GLideN64 on GLES3. Pinned upstream commit; part of the
# default H700 core matrix (package/libretro/README.md).
LIBRETRO_MUPEN64PLUS_NEXT_VERSION = 12edd2c74a517ff86dfa8cfc71ad75e4c10486d5
LIBRETRO_MUPEN64PLUS_NEXT_SITE = $(call github,libretro,mupen64plus-libretro-nx,$(LIBRETRO_MUPEN64PLUS_NEXT_VERSION))
LIBRETRO_MUPEN64PLUS_NEXT_LICENSE = GPL-2.0
LIBRETRO_MUPEN64PLUS_NEXT_LICENSE_FILES = LICENSE
LIBRETRO_MUPEN64PLUS_NEXT_DEPENDENCIES = libgles libretro-core-info

define LIBRETRO_MUPEN64PLUS_NEXT_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(LIBRETRO_CORE_ENV) $(MAKE) -C $(@D) -f Makefile \
		$(LIBRETRO_CORE_MAKE_OPTS) LD="$(TARGET_CXX)" FORCE_GLES3=1 ARCH=aarch64
endef

# Bundled as Mupen64Plus-Next-nuubOS (mupen64plus_next_nuubos): patch 0001
# adds the 400x300 and 480x360 GLideN64 resolutions, 0002 the
# library_name; upstream Mupen64Plus-Next remains a download.
define LIBRETRO_MUPEN64PLUS_NEXT_INSTALL_TARGET_CMDS
	$(call LIBRETRO_NUUBOS_CORE_INSTALL,mupen64plus_next,mupen64plus_next_nuubos,$(@D)/mupen64plus_next_libretro.so)
endef

$(eval $(generic-package))
