################################################################################
#
# libretro-vice
#
################################################################################

# VICE (Commodore 64, 128, Plus/4, VIC-20): one core per machine. Pinned upstream commit; part of the
# default H700 core matrix (package/libretro/README.md).
LIBRETRO_VICE_VERSION = f63b56688f3133a2bb17499db9eebb7ab81df5f5
LIBRETRO_VICE_SITE = $(call github,libretro,vice-libretro,$(LIBRETRO_VICE_VERSION))
LIBRETRO_VICE_LICENSE = GPL-2.0
LIBRETRO_VICE_LICENSE_FILES = COPYING

LIBRETRO_VICE_EMUTYPES = x64 x128 xplus4 xvic

# The Commodore ROMs are not redistributable: removed from the build tree
# and from the source archive legal-info publishes. Users put them in
# /userdata/bios/vice/<machine>/ (bios.conf). scripts/strip-commodore-roms.sh.
define LIBRETRO_VICE_STRIP_ROMS
	$(BR2_EXTERNAL_NUUBOS_PATH)/scripts/strip-commodore-roms.sh $(@D)
endef
LIBRETRO_VICE_POST_PATCH_HOOKS += LIBRETRO_VICE_STRIP_ROMS

# legal-info hard-links the download: write a stripped archive in its place.
define LIBRETRO_VICE_LEGAL_STRIP_ROMS
	rm -rf $(LIBRETRO_VICE_DIR)/.legal-strip
	mkdir -p $(LIBRETRO_VICE_DIR)/.legal-strip
	$(TAR) -xzf $(LIBRETRO_VICE_REDIST_SOURCES_DIR)/$(LIBRETRO_VICE_SOURCE) -C $(LIBRETRO_VICE_DIR)/.legal-strip
	$(BR2_EXTERNAL_NUUBOS_PATH)/scripts/strip-commodore-roms.sh $(LIBRETRO_VICE_DIR)/.legal-strip/*
	rm -f $(LIBRETRO_VICE_REDIST_SOURCES_DIR)/$(LIBRETRO_VICE_SOURCE)
	$(TAR) -czf $(LIBRETRO_VICE_REDIST_SOURCES_DIR)/$(LIBRETRO_VICE_SOURCE) \
		-C $(LIBRETRO_VICE_DIR)/.legal-strip .
	rm -rf $(LIBRETRO_VICE_DIR)/.legal-strip
endef
LIBRETRO_VICE_POST_LEGAL_INFO_HOOKS += LIBRETRO_VICE_LEGAL_STRIP_ROMS

# The objects depend on EMUTYPE: clean between machines.
define LIBRETRO_VICE_BUILD_CMDS
	mkdir -p $(@D)/nuubos-out
	for t in $(LIBRETRO_VICE_EMUTYPES); do \
		$(TARGET_MAKE_ENV) $(LIBRETRO_CORE_ENV) $(MAKE) -C $(@D) -f Makefile \
			$(LIBRETRO_CORE_MAKE_OPTS) LD="$(TARGET_CC)" EMUTYPE=$$t clean && \
		$(TARGET_MAKE_ENV) $(LIBRETRO_CORE_ENV) $(MAKE) -C $(@D) -f Makefile \
			$(LIBRETRO_CORE_MAKE_OPTS) LD="$(TARGET_CC)" EMUTYPE=$$t && \
		cp $(@D)/vice_$${t}_libretro.so $(@D)/nuubos-out/ || exit 1; \
	done
endef

define LIBRETRO_VICE_INSTALL_TARGET_CMDS
	for t in $(LIBRETRO_VICE_EMUTYPES); do \
		$(INSTALL) -D -m 0755 $(@D)/nuubos-out/vice_$${t}_libretro.so \
			$(TARGET_DIR)/usr/lib/libretro/vice_$${t}_libretro.so || exit 1; \
	done
endef

$(eval $(generic-package))
