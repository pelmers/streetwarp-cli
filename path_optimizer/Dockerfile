FROM lambci/lambda:python3.8

FROM lambci/lambda-base-2:build

ENV PATH=/var/lang/bin:$PATH \
    LD_LIBRARY_PATH=/var/lang/lib:$LD_LIBRARY_PATH \
    PKG_CONFIG_PATH=/var/lang/lib/pkgconfig:/usr/lib64/pkgconfig:/usr/share/pkgconfig
COPY --from=0 /var/lang /var/lang

RUN yum update -y
RUN yum install -y git cmake gcc-c++ gcc chrpath wget

RUN ln -s /usr/include/locale.h /usr/include/xlocale.h && /var/lang/bin/pip3 install numpy

RUN python3 --version

RUN mkdir -p /opt

RUN wget -q https://github.com/opencv/opencv/archive/3.4.13.zip -O ocv.zip && \
  unzip ocv.zip && rm ocv.zip && \
  wget -q https://github.com/opencv/opencv_contrib/archive/3.4.13.zip -O ocvc.zip && \
  unzip ocvc.zip && rm ocvc.zip 

RUN cd opencv-3.4.13 && mkdir build && cd build && \
  cmake -D CMAKE_BUILD_TYPE=RELEASE \
    -D CMAKE_C_COMPILER=/usr/bin/clang \
    -D CMAKE_CXX_COMPILER=/usr/bin/clang++ \
    -D CMAKE_CXX_FLAGS="-Os -s -ffunction-sections -fdata-sections -Wl,--gc-sections -Wl,-s" \
    -D CMAKE_INSTALL_PREFIX=/opt/ \
    -D INSTALL_PYTHON_EXAMPLES=OFF \
    -D BUILD_LIST="python3,flann,imgcodecs,imgproc,xfeatures2d,calib3d" \
    -D INSTALL_C_EXAMPLES=OFF \
    -D WITH_FFMPEG=OFF \
    -D WITH_TBB=ON \
    -D OPENCV_EXTRA_MODULES_PATH=../../opencv_contrib-3.4.13/modules \
    -D PYTHON3_EXECUTABLE=/var/lang/bin/python3 \
    -D PYTHON3_INCLUDE_DIR=/var/lang/include/python3.8 \
    -D PYTHON3_LIBRARY=/var/lang/lib/libpython3.8.so \
    ..

RUN cd opencv-3.4.13/build && make -j$(nproc) && make install && cd .. && rm -rf build

RUN pip3 uninstall -y numpy && pip install numpy --target="/opt/lib/python3.8/site-packages"
RUN rm -r /opt/bin /opt/share /opt/include
RUN cd /opt/lib/python3.8/site-packages && mv cv2/python-3.8/cv2.cpython-38-x86_64-linux-gnu.so cv2.so && rm -r cv2

ENV PYTHONPATH="/opt/lib/python3.8/site-packages:${PYTHONPATH}"
RUN python3 -c 'import numpy; numpy.identity(3); print("Python: import numpy - SUCCESS")'
RUN python3 -c 'import cv2; from cv2 import imread; cv2.xfeatures2d.BriefDescriptorExtractor_create(); print("Python: import cv2 - SUCCESS")'

RUN cd /opt && tar -cf target.tar lib64 lib