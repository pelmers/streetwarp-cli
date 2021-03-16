FROM lambci/lambda:nodejs12.x

FROM lambci/lambda-base-2:build

RUN yum update -y
RUN yum install -y git cmake gcc-c++ gcc python3-devel chrpath python37 wget

RUN ln -s /usr/include/locale.h /usr/include/xlocale.h && pip3 install numpy

RUN python3 --version

RUN mkdir -p /opt
WORKDIR /opt

RUN wget -q https://github.com/opencv/opencv/archive/3.4.13.zip -O ocv.zip && \
  unzip ocv.zip && rm ocv.zip && \
  wget -q https://github.com/opencv/opencv_contrib/archive/3.4.13.zip -O ocvc.zip && \
  unzip ocvc.zip && rm ocvc.zip 

RUN cd opencv-3.4.13 && mkdir build && cd build && \
  cmake -D CMAKE_BUILD_TYPE=RELEASE \
    -D CMAKE_C_COMPILER=/usr/bin/clang \
    -D CMAKE_CXX_COMPILER=/usr/bin/clang++ \
    -D CMAKE_INSTALL_PREFIX=/usr/local/ \
    -D INSTALL_PYTHON_EXAMPLES=OFF \
    -D BUILD_LIST="python3,flann,imgproc,xfeatures2d,calib3d" \
    -D INSTALL_C_EXAMPLES=OFF \
    -D WITH_FFMPEG=OFF \
    -D WITH_TBB=ON \
    -D OPENCV_EXTRA_MODULES_PATH=/opt/opencv_contrib-3.4.13/modules \
    -D PYTHON3_EXECUTABLE=/usr/bin/python3 \
    -D PYTHON3_LIBRARY=/usr/lib64/libpython3.7m.so \
    ..

RUN cd opencv-3.4.13/build && make -j$(nproc) && make install && cd .. && rm -rf build
RUN find /usr/local/lib/python3.7/site-packages/cv2

RUN python3 -c 'import sys; print(sys.path)'
RUN python3 -c 'import cv2; cv2.xfeatures2d.BriefDescriptorExtractor_create(); print("Python: import cv2 - SUCCESS")'