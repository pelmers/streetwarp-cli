FROM lambci/lambda:nodejs12.x

FROM lambci/lambda-base-2:build

RUN yum update -y
RUN yum install -y git cmake gcc-c++ gcc python3-devel chrpath wget

RUN ln -s /usr/include/locale.h /usr/include/xlocale.h && pip3 install numpy

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
    -D CMAKE_INSTALL_PREFIX=/opt/ \
    -D CMAKE_BUILD_RPATH_USE_ORIGIN=true \
    -D INSTALL_PYTHON_EXAMPLES=OFF \
    -D BUILD_LIST="python3,flann,imgproc,xfeatures2d,calib3d" \
    -D INSTALL_C_EXAMPLES=OFF \
    -D WITH_FFMPEG=OFF \
    -D WITH_TBB=ON \
    -D OPENCV_EXTRA_MODULES_PATH=../../opencv_contrib-3.4.13/modules \
    -D PYTHON3_EXECUTABLE=/usr/bin/python3 \
    -D PYTHON3_LIBRARY=/usr/lib64/libpython3.7m.so \
    ..

RUN cd opencv-3.4.13/build && make -j$(nproc) && make install && cd .. && rm -rf build

RUN pip3 uninstall -y numpy && pip install numpy --target="/opt/lib/python3.7/site-packages"
RUN rm -r /opt/bin /opt/share /opt/include
RUN cd /opt/lib/python3.7/site-packages && mv cv2/python-3.7/cv2.cpython-37m-x86_64-linux-gnu.so cv2.so && rm -r cv2
ENV PYTHONPATH="/opt/lib/python3.7/site-packages:${PYTHONPATH}"
RUN python3 -c 'import numpy; numpy.identity(3); print("Python: import numpy - SUCCESS")'
RUN python3 -c 'import cv2; cv2.xfeatures2d.BriefDescriptorExtractor_create(); print("Python: import cv2 - SUCCESS")'

RUN wget https://github.com/NixOS/patchelf/archive/0.12.tar.gz && tar -xf 0.12.tar.gz
RUN cd patchelf-0.12 && ./bootstrap.sh && ./configure --prefix=/opt && make && make check && make install
RUN cd /opt && tar -cf target.tar lib lib64 bin