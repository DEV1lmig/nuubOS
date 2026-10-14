################################################################################
#
# libsmb2
#
################################################################################

LIBSMB2_VERSION = 6.2
LIBSMB2_SITE = $(call github,sahlberg,libsmb2,libsmb2-$(LIBSMB2_VERSION))
# The library (lib/, include/) is LGPL-2.1+; the examples (BSD-2-Clause)
# are not built.
LIBSMB2_LICENSE = LGPL-2.1+
LIBSMB2_LICENSE_FILES = COPYING LICENCE-LGPL-2.1.txt
LIBSMB2_INSTALL_STAGING = YES
# NTLM only: no Kerberos/GSSAPI (libkrb5 is in the image for Steam Link,
# but share listing never needs it).
LIBSMB2_CONF_OPTS = \
	-DBUILD_SHARED_LIBS=ON \
	-DENABLE_EXAMPLES=OFF \
	-DENABLE_LIBKRB5=OFF \
	-DENABLE_GSSAPI=OFF

$(eval $(cmake-package))
